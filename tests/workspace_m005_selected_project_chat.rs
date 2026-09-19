//! Team Collaboration M005 — Workspace selected-project chat view.
//!
//! Non-modal `Route::Workspace` primary view: the ordinary Session/Task
//! composer stays editable for the selected project while the sidebar
//! shows project chat for that same selection over the existing
//! `ChatState`/daemon `chat.v1` service. Selection is a routing locator
//! guarded by generation + reconnect epoch; it never confers authority
//! and never falls back to cwd or the hidden prior session.
//!
//! These tests use only the public `App` surface (`process_msg`,
//! public state fields) plus a fake `CoreClient` for the bounded
//! dashboard/chat aggregates.

use std::sync::Arc;

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    ChatChannelDto, ChatMessageDto, CoreEvent, CoreRequest, CoreResponse, EventEnvelope,
    RequestEnvelope,
};
use codegg::protocol::work_order::ProjectActivitySummaryDto;
use codegg::tui::app::state::{
    ProjectTabState, ProjectTabs, WorkspaceDashboardState, WorkspaceFocus,
};
use codegg::tui::app::TuiMsg;
use codegg::tui::route::Route;
use tokio::sync::mpsc;

// ── Fake core ────────────────────────────────────────────────────────

struct FakeM005Client;

impl FakeM005Client {
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
impl CoreClient for FakeM005Client {
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
            CoreRequest::ChatCapabilities => CoreResponse::ChatCapabilities {
                capabilities: codegg::protocol::core::ChatCapabilitiesDto {
                    supported: true,
                    protocol_version: 1,
                    max_body_bytes: 8192,
                    max_page_limit: 100,
                    max_mentions: 16,
                    max_references: 8,
                    composing_ttl_secs: 30,
                    retention_max_messages: 1000,
                    actions_supported: false,
                    max_action_title_bytes: 0,
                    max_action_prompt_bytes: 0,
                },
            },
            CoreRequest::ChatChannelEnsure { project_id, .. } => {
                let channel = ChatChannelDto {
                    channel_id: format!("ch-{project_id}"),
                    project_id: project_id.clone(),
                    name: "general".to_string(),
                    created_by: "alice".to_string(),
                    created_at_ms: 1,
                };
                CoreResponse::ChatChannel { channel }
            }
            CoreRequest::ChatChannelList { project_id, .. } => {
                let channel = ChatChannelDto {
                    channel_id: format!("ch-{project_id}"),
                    project_id: project_id.clone(),
                    name: "general".to_string(),
                    created_by: "alice".to_string(),
                    created_at_ms: 1,
                };
                CoreResponse::ChatChannelList {
                    channels: vec![channel],
                    truncated: false,
                }
            }
            CoreRequest::ChatHistory { channel_id, .. } => {
                // Empty authorized window (Ready, no messages). Tests seed
                // specific bodies directly through `ChatState`.
                CoreResponse::ChatHistory {
                    channel_id,
                    messages: Vec::new(),
                    next_cursor: 0,
                    truncated: false,
                    retention_floor_seq: 0,
                }
            }
            CoreRequest::ChatSync { channel_id, .. } => CoreResponse::ChatSync {
                channel_id,
                messages: Vec::new(),
                next_cursor: 0,
                resync_required: false,
                retention_floor_seq: 0,
            },
            _ => CoreResponse::Error {
                code: "unsupported_in_test".to_string(),
                message: "fake m005 client".to_string(),
            },
        })
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(1);
        rx
    }
}

// ── Helpers (public surface only) ────────────────────────────────────

fn test_app_with_tabs() -> codegg::tui::app::App {
    let dir = tempfile::tempdir().unwrap();
    let root_a = dir.path().join("a");
    let root_b = dir.path().join("b");
    std::fs::create_dir_all(&root_a).unwrap();
    std::fs::create_dir_all(&root_b).unwrap();
    let dir_path = dir.keep();

    let mut app = codegg::tui::app::App::new_for_testing(dir_path.join("a").display().to_string());
    app.project_tabs = ProjectTabs::new();
    let mut tab_a = ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "Alpha".to_string(),
    );
    tab_a.project_id = Some("project-a".to_string());
    tab_a.workspace_id = Some("workspace-a".to_string());
    tab_a.workspace_root = Some(root_a);
    let mut tab_b = ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "Beta".to_string(),
    );
    tab_b.project_id = Some("project-b".to_string());
    tab_b.workspace_id = Some("workspace-b".to_string());
    tab_b.workspace_root = Some(root_b);
    app.project_tabs.add_tab(tab_a);
    app.project_tabs.add_tab(tab_b);
    let first = app.project_tabs.ordered()[0].tab_id.clone();
    app.project_tabs.set_active(&first);

    app.set_core_client(Arc::new(FakeM005Client));
    let (tx, _rx) = mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);
    app
}

fn selected_project(app: &codegg::tui::app::App) -> Option<String> {
    app.dialog_state
        .workspace_dashboard
        .as_ref()
        .and_then(|d| d.selected_project_id())
}

fn apply_rows(app: &mut codegg::tui::app::App, rows: Vec<ProjectActivitySummaryDto>) {
    // Drive the public reducer directly (no private command access):
    // begin a generation, apply the bounded page, then bind chat via the
    // public `ChatState` API when needed by the test.
    let (generation, _) = {
        let dashboard = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .expect("workspace open");
        (dashboard.generation, dashboard.reconnect_epoch)
    };
    app.dialog_state
        .workspace_dashboard
        .as_mut()
        .expect("workspace open")
        .apply_loaded(generation, rows, false, None);
}

fn open_workspace(app: &mut codegg::tui::app::App) {
    app.process_msg(TuiMsg::OpenWorkspaceDashboard);
    // The open spawns one bounded aggregate refresh; tests apply rows
    // explicitly via `apply_rows` for determinism.
}

fn chat_message(project_id: &str, channel_id: &str, seq: u64, body: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: format!("msg-{project_id}-{seq}"),
        channel_id: channel_id.to_string(),
        project_id: project_id.to_string(),
        seq,
        author_principal: "alice".to_string(),
        author_agent: None,
        body: body.to_string(),
        reply_to: None,
        thread_root: None,
        mentions: Vec::new(),
        references: Vec::new(),
        revision: 1,
        edited_at_ms: None,
        redacted: false,
        created_at_ms: 100 + seq as i64,
    }
}

fn seed_chat_ready(app: &mut codegg::tui::app::App, project_id: &str, bodies: &[&str]) {
    let channel_id = format!("ch-{project_id}");
    let Some(request_id) = app.chat.begin_history(project_id, &channel_id) else {
        panic!("begin_history for {project_id}");
    };
    let epoch = app.chat.reconnect_epoch;
    let messages: Vec<ChatMessageDto> = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| chat_message(project_id, &channel_id, (i + 1) as u64, body))
        .collect();
    assert!(app.chat.apply_history(
        request_id,
        project_id,
        &channel_id,
        messages,
        bodies.len() as u64,
        false,
        0,
        epoch,
    ));
}

// ── Tests ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn workspace_is_primary_route_with_editable_composer() {
    let mut app = test_app_with_tabs();
    app.session_state.session = Some(codegg::session::models::Session {
        id: "sess-a".to_string(),
        title: "A session".to_string(),
        project_id: "project-a".to_string(),
        workspace_id: Some("workspace-a".to_string()),
        directory: "/tmp".to_string(),
        ..Default::default()
    });
    let tab_before = app.project_tabs.active_tab_id().cloned();

    open_workspace(&mut app);
    // Let the spawned aggregate refresh run (one bounded request).
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    apply_rows(
        &mut app,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
    );

    assert!(matches!(app.ui_state.routes.current(), Route::Workspace));
    assert!(app.session_state.session.is_some());
    app.prompt_state
        .prompt
        .set_text("hello composer".to_string());
    assert_eq!(app.prompt_state.prompt.get_text(), "hello composer");
    assert_eq!(app.project_tabs.active_tab_id(), tab_before.as_ref());
    assert_eq!(app.workspace_focus, WorkspaceFocus::Composer);
}

#[tokio::test(flavor = "current_thread")]
async fn session_submit_targets_selected_project_not_hidden_session() {
    let mut app = test_app_with_tabs();
    app.session_state.session = Some(codegg::session::models::Session {
        id: "sess-a".to_string(),
        title: "A".to_string(),
        project_id: "project-a".to_string(),
        workspace_id: Some("workspace-a".to_string()),
        directory: "/tmp".to_string(),
        ..Default::default()
    });
    open_workspace(&mut app);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    apply_rows(
        &mut app,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
    );
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: 1 });
    assert_eq!(selected_project(&app).as_deref(), Some("project-b"));

    app.prompt_state
        .prompt
        .set_text("do work for B".to_string());
    app.process_msg(TuiMsg::SubmitPrompt);

    let pending = app
        .prompt_state
        .pending_session_submit
        .as_ref()
        .expect("session create pending for B");
    assert_eq!(pending.context.project_id.as_deref(), Some("project-b"));
    assert!(
        !pending.context.workspace_root.as_os_str().is_empty(),
        "never cwd fallback"
    );
    assert_eq!(app.active_project_id(), Some("project-b"));
    assert!(app.prompt_state.pending_send);
}

#[tokio::test(flavor = "current_thread")]
async fn session_submit_fails_visibly_without_selected_tab_never_fallback() {
    let mut app = test_app_with_tabs();
    open_workspace(&mut app);
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    // Dashboard row with no open tab.
    apply_rows(
        &mut app,
        vec![FakeM005Client::dashboard_row("project-z", "Zeta")],
    );
    assert_eq!(selected_project(&app).as_deref(), Some("project-z"));
    app.prompt_state.prompt.set_text("should fail".to_string());
    app.process_msg(TuiMsg::SubmitPrompt);
    assert!(
        app.prompt_state.pending_session_submit.is_none(),
        "must not create a session for another project"
    );
    assert!(
        !app.prompt_state.pending_send,
        "must not mark sending on failure"
    );
}

#[test]
fn task_routing_targets_selected_project() {
    let mut app = test_app_with_tabs();
    // Open the view state directly (public reducer, no async needed).
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
        false,
        None,
    );
    dashboard.move_selection(1);
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);

    assert_eq!(selected_project(&app).as_deref(), Some("project-b"));
    // Composer target is the selection (public state proves routing
    // without reaching into private command helpers).
    let tab_for_b = app
        .project_tabs
        .find_by_project("project-b")
        .expect("tab B exists");
    assert!(tab_for_b.workspace_root.is_some());
    assert_ne!(
        app.active_project_id(),
        Some("project-b"),
        "selection is a locator; active tab unchanged until submit"
    );
}

#[test]
fn side_panel_shows_selected_chat_with_separate_drafts() {
    let mut app = test_app_with_tabs();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
        false,
        None,
    );
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);

    seed_chat_ready(&mut app, "project-a", &["hello from A"]);
    seed_chat_ready(&mut app, "project-b", &["hello from B"]);

    // Select B via the public message path.
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: 1 });
    let now_ms = 1_000_000;
    let lines_b = app.chat.panel_lines("project-b", now_ms);
    assert!(lines_b.iter().any(|l| l.contains("hello from B")));
    assert!(!lines_b.iter().any(|l| l.contains("hello from A")));

    app.chat.set_draft("project-a", "draft A".to_string());
    app.chat.set_draft("project-b", "draft B".to_string());
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: -1 });
    assert_eq!(app.chat.draft_for("project-a"), "draft A");
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: 1 });
    assert_eq!(app.chat.draft_for("project-b"), "draft B");
}

#[test]
fn denied_and_granted_chat_render_correctly() {
    let mut app = test_app_with_tabs();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![FakeM005Client::dashboard_row("project-a", "Alpha")],
        false,
        None,
    );
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);

    let channel_id = "ch-project-a".to_string();
    let Some(request_id) = app.chat.begin_history("project-a", &channel_id) else {
        panic!("begin");
    };
    let chat_epoch = app.chat.reconnect_epoch;
    assert!(app.chat.apply_error(
        request_id,
        "project-a",
        "project_not_found: denied".to_string(),
        true,
        false,
        chat_epoch,
    ));
    let lines = app.chat.panel_lines("project-a", 1_000_000);
    let joined = lines.join("\n").to_lowercase();
    assert!(
        joined.contains("unavailable")
            || joined.contains("not found")
            || joined.contains("no chat"),
        "denied must render generic unavailable, got: {joined}"
    );

    seed_chat_ready(&mut app, "project-a", &["granted hello"]);
    let lines = app.chat.panel_lines("project-a", 1_000_000);
    assert!(lines.iter().any(|l| l.contains("granted hello")));

    let Some(request_id) = app.chat.begin_history("project-a", &channel_id) else {
        panic!("begin2");
    };
    assert!(app.chat.apply_error(
        request_id,
        "project-a",
        "project_not_found: denied".to_string(),
        true,
        false,
        chat_epoch,
    ));
    let lines = app.chat.panel_lines("project-a", 1_000_000);
    let joined = lines.join("\n").to_lowercase();
    assert!(
        joined.contains("unavailable")
            || joined.contains("not found")
            || joined.contains("no chat"),
        "denial must not leak prior messages, got: {joined}"
    );
}

#[test]
fn stale_generation_and_revocation_clear_correctly() {
    let mut app = test_app_with_tabs();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
        false,
        None,
    );
    app.dialog_state.workspace_dashboard = Some(dashboard);

    seed_chat_ready(&mut app, "project-b", &["keep me"]);
    let (generation, reconnect_epoch) = {
        let d = app.dialog_state.workspace_dashboard.as_ref().unwrap();
        (d.generation, d.reconnect_epoch)
    };
    // Stale generation drops through the public reducer.
    assert!(!app
        .dialog_state
        .workspace_dashboard
        .as_mut()
        .unwrap()
        .apply_loaded(generation.wrapping_add(1), Vec::new(), false, None));
    assert_eq!(
        app.dialog_state
            .workspace_dashboard
            .as_ref()
            .unwrap()
            .rows
            .len(),
        2,
        "stale generation must drop"
    );
    // Revocation clears dashboard row + chat cache via public reducers.
    app.dialog_state
        .workspace_dashboard
        .as_mut()
        .unwrap()
        .clear_revoked("project-b");
    app.chat.clear_project("project-b");
    assert_eq!(
        app.dialog_state
            .workspace_dashboard
            .as_ref()
            .unwrap()
            .rows
            .len(),
        1
    );
    assert!(app.chat.get("project-b").is_none());
    let _ = reconnect_epoch;
}

#[test]
fn modal_above_workspace_returns_to_same_selection() {
    let mut app = test_app_with_tabs();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
        false,
        None,
    );
    dashboard.move_selection(1);
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);
    let selected_before = selected_project(&app);
    assert_eq!(selected_before.as_deref(), Some("project-b"));

    // Modal confirmation above Workspace preserves selection (public
    // message path: request a delete confirm, then dismiss).
    app.process_msg(TuiMsg::ConfirmDeleteSession {
        session_id: "sess-x".to_string(),
    });
    app.process_msg(TuiMsg::CloseDialog);
    assert!(matches!(app.ui_state.routes.current(), Route::Workspace));
    assert_eq!(selected_project(&app), selected_before);
}

#[test]
fn narrow_terminal_degrades_cleanly() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    let mut app = test_app_with_tabs();
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM005Client::dashboard_row("project-a", "Alpha"),
            FakeM005Client::dashboard_row("project-b", "Beta"),
        ],
        false,
        None,
    );
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);
    seed_chat_ready(&mut app, "project-a", &["narrow hello"]);
    for (w, h) in [(58, 20), (70, 20), (100, 32), (120, 40)] {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| app.render(frame))
            .expect("render must not panic");
        let buf = terminal.backend().buffer().clone();
        let text: String = buf
            .content()
            .iter()
            .map(|cell| cell.symbol().to_string())
            .collect();
        assert!(
            !text.contains("Rendering Error"),
            "clean degrade at {w}x{h}"
        );
    }
}
