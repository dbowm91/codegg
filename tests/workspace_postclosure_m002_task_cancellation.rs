//! Team Collaboration Post-Closure M002 — Workspace task cancellation ownership.
//!
//! Regression for the over-broad `leave_workspace_view` cancellation that
//! aborted every `TuiTaskKind::Command` task when the user exited the
//! non-modal `Route::Workspace` view. `Command` is shared by chat, team
//! administration, control, provider, project/session, diagnostics, and
//! WorkOrder flows; only Workspace-owned dashboard refresh/expand work
//! (now `TuiTaskKind::Workspace`) may be cancelled on close.
//!
//! These tests drive only the public `App` surface (`process_msg`,
//! `on_key`, public state fields) plus a fake `CoreClient`.

use std::sync::Arc;

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope,
};
use codegg::protocol::work_order::ProjectActivitySummaryDto;
use codegg::tui::app::state::{ProjectTabId, ProjectTabState, ProjectTabs};
use codegg::tui::app::TuiMsg;
use codegg::tui::route::Route;
use codegg::tui::task_lifecycle::TuiTaskKind;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

// ── Fake core ──────────────────────────────────────────────────────────

struct FakeM002Client;

impl FakeM002Client {
    fn dashboard_row(project_id: &str, display_name: &str) -> ProjectActivitySummaryDto {
        ProjectActivitySummaryDto {
            project_id: project_id.to_string(),
            display_name: display_name.to_string(),
            lifecycle: "active".to_string(),
            running_session_count: 1,
            running_work_order_count: 0,
            waiting_work_order_count: 1,
            future_work_order_count: 1,
            needs_attention_count: 0,
            pending_permission_count: 0,
            pending_question_count: 0,
            last_activity_at: Some(7),
            coarse_status_code: "waiting".to_string(),
            counts_visible: true,
        }
    }
}

#[async_trait]
impl CoreClient for FakeM002Client {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        Ok(match request.payload {
            CoreRequest::WorkspaceDashboard { .. } => CoreResponse::WorkspaceDashboard {
                rows: vec![
                    Self::dashboard_row("project-a", "Alpha"),
                    Self::dashboard_row("project-b", "Beta"),
                ],
                next_cursor: None,
                truncated: false,
            },
            _ => CoreResponse::Error {
                code: "unsupported_in_test".to_string(),
                message: "fake m002 client".to_string(),
            },
        })
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}

// ── Helpers (public surface only) ──────────────────────────────────────

fn test_app_with_tabs() -> codegg::tui::app::App {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("a");
    let root_b = dir.path().join("b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let dir_path = dir.keep();

    let mut app = codegg::tui::app::App::new_for_testing(dir_path.join("a").display().to_string());
    app.project_tabs = ProjectTabs::new();
    let mut tab_a = ProjectTabState::empty(ProjectTabId::new(), "Alpha".to_string());
    tab_a.project_id = Some("project-a".to_string());
    tab_a.workspace_id = Some("workspace-a".to_string());
    tab_a.workspace_root = Some(root_a);
    let mut tab_b = ProjectTabState::empty(ProjectTabId::new(), "Beta".to_string());
    tab_b.project_id = Some("project-b".to_string());
    tab_b.workspace_id = Some("workspace-b".to_string());
    tab_b.workspace_root = Some(root_b);
    app.project_tabs.add_tab(tab_a);
    app.project_tabs.add_tab(tab_b);
    let first = app.project_tabs.ordered()[0].tab_id.clone();
    app.project_tabs.set_active(&first);

    app.set_core_client(Arc::new(FakeM002Client));
    let (tx, _rx) = mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);
    app
}

fn open_workspace(app: &mut codegg::tui::app::App) {
    app.process_msg(TuiMsg::OpenWorkspaceDashboard);
}

fn press_esc(app: &mut codegg::tui::app::App) {
    app.on_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
}

async fn pending() {
    std::future::pending::<()>().await;
}

fn active_names(app: &codegg::tui::app::App) -> Vec<&'static str> {
    app.task_registry
        .iter()
        .map(|(_, record)| record.name)
        .collect()
}

fn count_kind(app: &codegg::tui::app::App, kind: TuiTaskKind) -> usize {
    app.task_registry
        .iter()
        .filter(|(_, record)| record.kind == kind)
        .count()
}

fn apply_rows(app: &mut codegg::tui::app::App, rows: Vec<ProjectActivitySummaryDto>) {
    let generation = app
        .dialog_state
        .workspace_dashboard
        .as_ref()
        .expect("workspace open")
        .generation;
    app.dialog_state
        .workspace_dashboard
        .as_mut()
        .expect("workspace open")
        .apply_loaded(generation, rows, false, None);
}

// ── Tests ──────────────────────────────────────────────────────────────

/// Core M002 regression: leaving Workspace cancels only Workspace-owned
/// tasks. Unrelated generic `Command` tasks (team admin, chat, provider,
/// project/session) keep running and cancellation accounting covers only
/// the Workspace-owned tasks.
#[tokio::test(flavor = "current_thread")]
async fn unrelated_command_tasks_survive_workspace_close() {
    let mut app = test_app_with_tabs();
    open_workspace(&mut app);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    assert!(matches!(app.ui_state.routes.current(), Route::Workspace));
    assert!(app.dialog_state.workspace_dashboard.is_some());

    // Co-schedule unrelated generic work alongside Workspace-owned work.
    // The pending futures stay in the registry until cancelled/reaped.
    app.task_registry
        .spawn(TuiTaskKind::Command, "team_admin_refresh", pending());
    app.task_registry
        .spawn(TuiTaskKind::Command, "generic_chat_refresh", pending());
    app.task_registry
        .spawn(TuiTaskKind::Command, "provider_refresh", pending());
    app.task_registry
        .spawn(TuiTaskKind::Command, "project_session_command", pending());
    app.task_registry.spawn(
        TuiTaskKind::Workspace,
        "workspace_inflight_expand",
        pending(),
    );

    let workspace_before = count_kind(&app, TuiTaskKind::Workspace);
    assert!(
        workspace_before >= 1,
        "expected Workspace-owned work in flight, found {workspace_before}"
    );
    let command_before = count_kind(&app, TuiTaskKind::Command);
    assert!(
        command_before >= 4,
        "expected co-scheduled Command tasks, found {command_before}"
    );
    let cancelled_before = app.task_registry.cancelled_count();

    press_esc(&mut app);

    // View is closed through the route path.
    assert!(!matches!(app.ui_state.routes.current(), Route::Workspace));
    assert!(app.dialog_state.workspace_dashboard.is_none());

    // Only Workspace-owned tasks were cancelled.
    let cancelled_delta = app
        .task_registry
        .cancelled_count()
        .saturating_sub(cancelled_before);
    assert_eq!(
        cancelled_delta as usize, workspace_before,
        "cancellation accounting must cover exactly the Workspace-owned tasks"
    );
    assert_eq!(
        count_kind(&app, TuiTaskKind::Workspace),
        0,
        "no Workspace-owned task may survive close"
    );

    // Every unrelated Command task remains registered and unfinished.
    for name in [
        "team_admin_refresh",
        "generic_chat_refresh",
        "provider_refresh",
        "project_session_command",
    ] {
        assert!(
            active_names(&app).contains(&name),
            "unrelated Command task {name} must survive Workspace close"
        );
    }
    for (_, record) in app.task_registry.iter() {
        if record.kind == TuiTaskKind::Command {
            assert!(
                !record.is_finished(),
                "Command task {} must not be aborted by Workspace close",
                record.name
            );
        }
    }

    // Diagnostics stay truthful: Command work is still reported, and no
    // Workspace line lingers after its tasks were cancelled.
    let summary = app.task_registry.summary();
    assert!(
        summary.contains("Command:"),
        "task summary must still report surviving Command work: {summary}"
    );
    assert!(
        !summary.contains("Workspace:"),
        "task summary must not report Workspace work after close: {summary}"
    );
}

/// Workspace-owned refresh work is precisely cancelled on close and a
/// late completion can never repopulate the closed view: the dashboard
/// state is dropped and a reopen starts fresh with no resurrection.
#[tokio::test(flavor = "current_thread")]
async fn workspace_owned_work_cancelled_and_completion_stale_dropped() {
    let mut app = test_app_with_tabs();
    open_workspace(&mut app);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    apply_rows(
        &mut app,
        vec![
            FakeM002Client::dashboard_row("project-a", "Alpha"),
            FakeM002Client::dashboard_row("project-b", "Beta"),
        ],
    );
    assert_eq!(
        app.dialog_state
            .workspace_dashboard
            .as_ref()
            .unwrap()
            .rows
            .len(),
        2
    );

    // Capture the in-flight request fencing before close.
    let (old_generation, old_request_id) = {
        let dashboard = app.dialog_state.workspace_dashboard.as_mut().unwrap();
        let generation = dashboard.begin_refresh();
        let request_id = dashboard.request.begin();
        (generation, request_id)
    };

    press_esc(&mut app);
    assert!(app.dialog_state.workspace_dashboard.is_none());

    // Reopen: a brand-new dashboard owns fresh state; the old completion
    // has no target and the old request id can never finish there.
    open_workspace(&mut app);
    {
        let dashboard = app
            .dialog_state
            .workspace_dashboard
            .as_mut()
            .expect("workspace reopened");
        assert!(
            dashboard.rows.is_empty(),
            "reopened Workspace must not resurrect rows from the closed view"
        );
        assert!(
            dashboard.expanded_project_id.is_none(),
            "reopened Workspace must not resurrect inline expansion"
        );
        assert!(
            !dashboard.request.finish(old_request_id),
            "stale request id from the closed view must not finish"
        );
        assert_ne!(
            dashboard.generation, old_generation,
            "reopened Workspace must own a new generation"
        );
    }
    // No Workspace task leaked across the close/reopen boundary.
    app.task_registry.reap_finished();
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    let _ = old_generation;
}

/// Leaving Workspace clears only the panel binding/focus: per-project
/// chat drafts and cached chat state survive the close.
#[tokio::test(flavor = "current_thread")]
async fn chat_drafts_survive_workspace_close() {
    let mut app = test_app_with_tabs();
    app.chat
        .set_draft("project-a", "draft for alpha".to_string());
    app.chat
        .set_draft("project-b", "draft for beta".to_string());

    open_workspace(&mut app);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    apply_rows(
        &mut app,
        vec![
            FakeM002Client::dashboard_row("project-a", "Alpha"),
            FakeM002Client::dashboard_row("project-b", "Beta"),
        ],
    );

    press_esc(&mut app);
    assert!(app.dialog_state.workspace_dashboard.is_none());
    assert!(
        app.chat_panel_project.is_none(),
        "panel binding must clear on close"
    );
    assert_eq!(
        app.chat.draft_for("project-a"),
        "draft for alpha",
        "leaving Workspace must not destroy per-project chat drafts"
    );
    assert_eq!(
        app.chat.draft_for("project-b"),
        "draft for beta",
        "leaving Workspace must not destroy per-project chat drafts"
    );
}

/// Guard the invariants that must not regress: tab/session-scoped and
/// global shutdown cancellation keep working independently of the new
/// Workspace kind.
#[tokio::test(flavor = "current_thread")]
async fn scoped_and_shutdown_cancellation_still_work() {
    use codegg::tui::task_lifecycle::TuiTaskRegistry;

    let mut registry = TuiTaskRegistry::new();
    registry.spawn_with_scope(
        TuiTaskKind::Command,
        "tab_scoped",
        Some("tab-a".to_string()),
        None,
        None,
        pending(),
    );
    registry.spawn_with_scope(
        TuiTaskKind::Workspace,
        "workspace_global",
        None,
        None,
        None,
        pending(),
    );
    registry.spawn_with_scope(
        TuiTaskKind::Command,
        "session_scoped",
        None,
        Some("sess-1".to_string()),
        None,
        pending(),
    );
    assert_eq!(registry.active_count(), 3);

    assert_eq!(registry.cancel_for_tab("tab-a"), 1);
    assert_eq!(registry.cancel_for_session("sess-1"), 1);
    // The Workspace-owned task is untouched by tab/session proxies,
    // per the plan's prohibition on using them as close semantics.
    assert_eq!(registry.active_count(), 1);
    assert!(registry
        .iter()
        .all(|(_, r)| r.kind == TuiTaskKind::Workspace));

    registry.cancel_all();
    assert_eq!(registry.active_count(), 0);
    assert_eq!(registry.cancelled_count(), 3);
}
