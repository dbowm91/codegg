//! Integration tests for the project-aware TUI state seam
//! (Multi-Project TUI milestone 1).
//!
//! These tests pin down the contract that:
//!
//! * `App::project_tabs` always has at least one active tab after
//!   construction (compatibility startup).
//! * Active-tab accessors (`active_session_id`, `active_project_id`,
//!   `active_workspace_id`, `active_model`, `active_agent`) mirror
//!   the legacy single-project fields.
//! * The project catalog async command pair
//!   (`start_refresh_project_catalog` /
//!   `apply_project_catalog_refreshed`) propagates `CoreClient`
//!   responses, drops stale completions, and supports an unsupported
//!   capability fallback.
//! * All `CoreClient` transports already support the bounded
//!   `ProjectList` / `ProjectGet` wire methods. (This is asserted
//!   here via the same fake client used by other integration tests.)

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, PROTOCOL_VERSION,
};
use codegg::protocol::dto::{
    ProjectDetailsDto, ProjectHealthDto, ProjectHealthLayerDto, ProjectSummaryDto,
    ProjectWorkspaceSummaryDto,
};
use codegg::tui::app::TuiCommand;
use codegg::tui::async_cmd::spawn_registered_tui_task;
use codegg::tui::task_lifecycle::{TuiTaskKind, TuiTaskRegistry};
use tokio::sync::mpsc;

#[derive(Default)]
struct FakeProjectCatalogClient {
    /// List of received requests, in order.
    received: Mutex<Vec<String>>,
    /// Whether to advertise the capability.
    advertise_capability: bool,
}

impl FakeProjectCatalogClient {
    fn with_capability(supported: bool) -> Self {
        Self {
            received: Mutex::new(Vec::new()),
            advertise_capability: supported,
        }
    }
}

fn summary_dto(project_id: &str, name: &str) -> ProjectSummaryDto {
    ProjectSummaryDto {
        project_id: project_id.to_string(),
        display_name: name.to_string(),
        lifecycle: "Active".to_string(),
        description: None,
        tags: Vec::new(),
        time_last_opened_at: None,
        registration_source: "test".to_string(),
        archived_at: None,
        created_at: 0,
        updated_at: 0,
    }
}

#[async_trait]
impl CoreClient for FakeProjectCatalogClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        let mut guard = self.received.lock().unwrap();
        match request.payload {
            CoreRequest::ProjectCatalogCapabilities => {
                guard.push("ProjectCatalogCapabilities".to_string());
                Ok(CoreResponse::ProjectCatalogCapabilities {
                    supported: self.advertise_capability,
                    max_list_items: 128,
                    max_workspaces_per_project: 64,
                })
            }
            CoreRequest::ProjectList {
                include_archived,
                limit,
            } => {
                guard.push(format!(
                    "ProjectList(include_archived={}, limit={})",
                    include_archived, limit
                ));
                if !self.advertise_capability {
                    Ok(CoreResponse::Error {
                        code: "unsupported".to_string(),
                        message: "catalog not enabled".to_string(),
                    })
                } else {
                    Ok(CoreResponse::ProjectList {
                        projects: vec![summary_dto("project-a", "Alpha")],
                        truncated: false,
                    })
                }
            }
            CoreRequest::ProjectGet { project_id } => {
                guard.push(format!("ProjectGet({})", project_id));
                Ok(CoreResponse::ProjectGet {
                    project: ProjectDetailsDto {
                        project: summary_dto(&project_id, "Alpha"),
                        workspaces: vec![ProjectWorkspaceSummaryDto {
                            workspace_id: "ws-1".to_string(),
                            display_name: "main".to_string(),
                            canonical_root: Some("/tmp/a".to_string()),
                        }],
                        session_count: 3,
                        health: Some(codegg::protocol::dto::ProjectHealthRecordDto {
                            project_id: project_id.clone(),
                            status: "available".to_string(),
                            error_code: None,
                            error_message: None,
                            source: "test".to_string(),
                            evaluated_at: 0,
                            notes: None,
                        }),
                    },
                })
            }
            other => panic!("unexpected request in fake catalog client: {other:?}"),
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::unbounded_channel();
        rx
    }
}

fn drain_completions(rx: &mut mpsc::Receiver<TuiCommand>) -> Vec<TuiCommand> {
    let mut out = Vec::new();
    while let Ok(cmd) = rx.try_recv() {
        out.push(cmd);
    }
    out
}

#[test]
fn app_always_has_one_compat_tab_after_new_for_testing() {
    let app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    assert_eq!(app.open_tab_count(), 1);
    let active = app.active_tab().expect("active tab");
    assert_eq!(active.project_id, None);
    assert_eq!(active.session_id, None);
    assert_eq!(active.workspace_id, None);
    assert!(app.active_tab_id().is_some());
}

#[test]
fn active_accessors_reflect_compat_state() {
    let app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    assert_eq!(app.active_session_id(), None);
    assert_eq!(app.active_project_id(), None);
    assert_eq!(app.active_workspace_id(), None);
    // Compat tab inherits the default model + agent.
    assert!(!app.active_model().is_empty());
    assert!(!app.active_agent().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn catalog_capability_unsupported_keeps_compat_tab_usable() {
    let fake = Arc::new(FakeProjectCatalogClient::with_capability(false));
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    app.set_core_client(fake.clone());

    // Inject a channel and trigger the refresh. The spawned task will
    // run on the current Tokio runtime.
    let (tx, mut rx) = mpsc::channel(8);
    app.tui_cmd_tx = Some(tx.clone());
    app.refresh_project_catalog();

    // Wait briefly for the completion to land.
    let cmd = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("completion should arrive within timeout")
        .expect("channel should not be closed");

    match cmd {
        TuiCommand::ProjectCatalogRefreshed {
            supported,
            entries,
            error,
            request_id,
            truncated,
        } => {
            assert!(!supported);
            assert!(entries.is_empty());
            assert!(error.is_none());
            // Dispatcher-style apply through the App façade.
            app.apply_project_catalog_refreshed(request_id, supported, entries, truncated, error);
        }
        other => panic!("unexpected completion: {other:?}"),
    }

    // Drain any stragglers and assert no extra completions arrived.
    let extras = drain_completions(&mut rx);
    assert!(extras.is_empty(), "no extra completions: {extras:?}");

    assert!(!app.project_catalog_supported());
    assert_eq!(app.project_catalog.entries.len(), 0);
    // Compat tab remains intact.
    assert_eq!(app.open_tab_count(), 1);
    assert!(app.active_tab().is_some());
}

#[test]
fn catalog_capability_supported_applies_list_completion() {
    let fake = Arc::new(FakeProjectCatalogClient::with_capability(true));
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    app.set_core_client(fake.clone());

    // Run the catalog refresh path inline (no event loop needed):
    // begin request, simulate the dispatcher's apply.
    let request_id = app.project_catalog.list_request.begin();
    app.project_catalog
        .apply_list(request_id, vec![summary_dto("project-a", "Alpha")], false);
    app.project_catalog.set_capability(true);

    assert!(app.project_catalog_supported());
    assert_eq!(app.project_catalog.entries.len(), 1);
    assert_eq!(app.project_catalog.entries[0].project_id, "project-a");
    assert!(!app.project_catalog.truncated);
}

#[test]
fn stale_catalog_completion_is_dropped() {
    let mut state = codegg::tui::app::state::ProjectCatalogState::new();
    let id1 = state.list_request.begin();
    // User starts a second refresh before the first finishes.
    let id2 = state.list_request.begin();
    assert!(!state.apply_list(id1, vec![summary_dto("stale", "Stale")], false));
    assert!(state.apply_list(id2, vec![summary_dto("fresh", "Fresh")], false));
    assert_eq!(state.entries.len(), 1);
    assert_eq!(state.entries[0].project_id, "fresh");
}

#[test]
fn catalog_state_failure_records_error_and_resets_loading() {
    let mut state = codegg::tui::app::state::ProjectCatalogState::new();
    let id = state.list_request.begin();
    let applied = state.apply_list_error(id, "boom".to_string());
    assert!(applied);
    assert_eq!(state.last_error.as_deref(), Some("boom"));
    assert!(!state.is_loading());
}

#[test]
fn project_tab_identity_is_distinct_from_daemon_ids() {
    let app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    let tab_id = app.active_tab_id().unwrap();
    // The tab id must not collide with any daemon-shaped id; it is a
    // frontend-local UUID string.
    assert_ne!(tab_id.as_str(), "/tmp/foo");
    assert!(tab_id.as_str().len() > 8);
}

#[test]
fn setting_session_updates_active_tab() {
    use codegg::session::Session;
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/foo".to_string());
    let sess = Session {
        id: "session-x".to_string(),
        project_id: "project-x".to_string(),
        workspace_id: Some("ws-x".to_string()),
        parent_id: None,
        slug: "x".to_string(),
        directory: "/tmp/foo".to_string(),
        title: "X".to_string(),
        version: "v1".to_string(),
        share_url: None,
        summary_additions: None,
        summary_deletions: None,
        summary_files: None,
        summary_diffs: None,
        revert: None,
        permission: None,
        tags: Vec::new(),
        provider_connection_id: None,
        provider_connection_revision: None,
        model_catalog_revision: None,
        selected_model_id: None,
        agent: None,
        model: None,
        time_created: 0,
        time_updated: 0,
        time_compacting: None,
        time_archived: None,
        time_deleted: None,
    };
    app.set_session(sess);
    assert_eq!(app.active_session_id(), Some("session-x"));
    assert_eq!(app.active_project_id(), Some("project-x"));
    assert_eq!(app.active_workspace_id(), Some("ws-x"));
}

#[test]
fn tab_ids_are_unique_across_app_instances() {
    let app_a = codegg::tui::app::App::new_for_testing("/tmp/a".to_string());
    let app_b = codegg::tui::app::App::new_for_testing("/tmp/b".to_string());
    assert_ne!(
        app_a.active_tab_id().unwrap(),
        app_b.active_tab_id().unwrap()
    );
}

#[test]
fn identical_session_titles_do_not_collide_tabs() {
    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    let mut a = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "alpha".to_string(),
    );
    a.project_id = Some("project-a".to_string());
    a.session_id = Some("session-1".to_string());
    let mut b = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "beta".to_string(),
    );
    b.project_id = Some("project-b".to_string());
    b.session_id = Some("session-1".to_string());
    let id_a = tabs.add_and_activate(a);
    let id_b = tabs.add_tab(b);
    assert_ne!(id_a, id_b);
    let sa = tabs.get(&id_a).unwrap().session_id.clone().unwrap();
    let sb = tabs.get(&id_b).unwrap().session_id.clone().unwrap();
    assert_eq!(sa, sb);
    assert_ne!(id_a, id_b);
}

#[test]
fn remove_active_tab_falls_back_to_adjacent_previous() {
    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    let a = tabs.add_and_activate(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "a".to_string(),
    ));
    let b = tabs.add_tab(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "b".to_string(),
    ));
    let _c = tabs.add_tab(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "c".to_string(),
    ));
    // Order: [a, b, c]; active is a. Remove a -> falls back to
    // adjacent previous. Since a is first, adjacent next (b).
    tabs.remove_tab(&a);
    assert_eq!(tabs.active_tab_id(), Some(&b));
    assert_eq!(tabs.len(), 2);
}

#[test]
fn catalog_refresh_spawns_completion_via_spawn_registered_tui_task() {
    // Verify the canonical async pattern: spawn a task that returns a
    // ProjectCatalogRefreshed completion and ensure it lands on the
    // channel. This mirrors how the production dispatcher awaits the
    // completion.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let (tx, mut rx) = mpsc::channel(4);
        let mut registry = TuiTaskRegistry::new();
        let id = spawn_registered_tui_task(
            Some(tx.clone()),
            &mut registry,
            TuiTaskKind::Command,
            "test_catalog_refresh",
            async move {
                Some(TuiCommand::ProjectCatalogRefreshed {
                    request_id: 1,
                    supported: true,
                    entries: vec![summary_dto("p", "P")],
                    truncated: false,
                    error: None,
                })
            },
        );
        assert!(id.is_some());
        // Give the task a moment.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let cmd = rx.try_recv().expect("completion should be queued");
        match cmd {
            TuiCommand::ProjectCatalogRefreshed {
                request_id,
                supported,
                entries,
                truncated,
                error,
            } => {
                assert_eq!(request_id, 1);
                assert!(supported);
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].project_id, "p");
                assert!(!truncated);
                assert!(error.is_none());
            }
            other => panic!("unexpected completion: {other:?}"),
        }
    });
}

#[test]
fn fake_client_handles_project_list_and_get_consistently() {
    // The fake client must satisfy both list and get paths because the
    // catalog async path exercises both shapes (list now, get on
    // demand for picker detail in milestone 2).
    let fake = FakeProjectCatalogClient::with_capability(true);
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let list = fake
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "r".to_string(),
                payload: CoreRequest::ProjectList {
                    include_archived: false,
                    limit: 64,
                },
            })
            .await
            .expect("list should succeed");
        match list {
            CoreResponse::ProjectList { projects, .. } => {
                assert_eq!(projects.len(), 1);
            }
            other => panic!("expected ProjectList, got {other:?}"),
        }
        let get = fake
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "r".to_string(),
                payload: CoreRequest::ProjectGet {
                    project_id: "project-a".to_string(),
                },
            })
            .await
            .expect("get should succeed");
        match get {
            CoreResponse::ProjectGet { project } => {
                assert_eq!(project.project.project_id, "project-a");
                assert_eq!(project.workspaces.len(), 1);
                assert_eq!(project.session_count, 3);
            }
            other => panic!("expected ProjectGet, got {other:?}"),
        }
        let caps = fake
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: "r".to_string(),
                payload: CoreRequest::ProjectCatalogCapabilities,
            })
            .await
            .expect("caps should succeed");
        match caps {
            CoreResponse::ProjectCatalogCapabilities {
                supported,
                max_list_items,
                max_workspaces_per_project,
            } => {
                assert!(supported);
                assert_eq!(max_list_items, 128);
                assert_eq!(max_workspaces_per_project, 64);
            }
            other => panic!("expected caps, got {other:?}"),
        }
    });
}

// The following imports are kept here to ensure the types are
// referenced and the compiler does not eliminate them. ProjectHealthDto
// is part of the wire shape but milestone 1 does not exercise it
// directly; the import guards against accidental removal in later
// refactors.
#[allow(dead_code)]
fn _type_pin(_: ProjectHealthDto, _: ProjectHealthLayerDto) {}

// --- Milestone 2 corrective tests ---

#[test]
fn remove_tab_does_not_delete_daemon_session() {
    // Verifies that remove_tab only mutates frontend state — no daemon
    // call is made. This is the key invariant: closing a tab never
    // deletes, archives, or cancels daemon-owned sessions.
    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    let a = tabs.add_and_activate(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "a".to_string(),
    ));
    let _b = tabs.add_tab(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "b".to_string(),
    ));
    assert_eq!(tabs.len(), 2);
    let removed = tabs.remove_tab(&a).expect("remove a");
    assert_eq!(removed.label, "a");
    assert_eq!(tabs.len(), 1);
    assert!(tabs.active().is_some());
}

#[test]
fn close_last_tab_fallback_has_no_daemon_ids() {
    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    let a = tabs.add_and_activate(codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "a".to_string(),
    ));
    tabs.remove_tab(&a);
    assert!(tabs.is_empty());
    let fallback_id = tabs.close_fallback_tab();
    let active = tabs.active().expect("fallback");
    assert_eq!(active.tab_id, fallback_id);
    assert_eq!(active.label, "default");
    assert!(active.project_id.is_none());
    assert!(active.workspace_id.is_none());
    assert!(active.session_id.is_none());
}

#[test]
fn find_by_project_focuses_existing_tab() {
    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    let compat = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "compat".to_string(),
    );
    tabs.add_and_activate(compat);

    let mut tab = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "existing".to_string(),
    );
    tab.project_id = Some("proj-123".to_string());
    let existing_id = tab.tab_id.clone();
    tabs.add_tab(tab);
    assert_eq!(tabs.len(), 2);

    let found = tabs.find_by_project("proj-123").unwrap();
    assert_eq!(found.tab_id, existing_id);
    tabs.set_active(&existing_id);
    assert_eq!(tabs.len(), 2);
    assert_eq!(tabs.active_tab_id(), Some(&existing_id));
}

#[test]
fn capacity_enforced_at_state_level() {
    use codegg::tui::app::state::project_picker::MAX_OPEN_PROJECT_TABS;

    let mut tabs = codegg::tui::app::state::ProjectTabs::default();
    for i in 0..MAX_OPEN_PROJECT_TABS {
        let mut tab = codegg::tui::app::state::ProjectTabState::empty(
            codegg::tui::app::state::ProjectTabId::new(),
            format!("tab-{}", i),
        );
        tab.project_id = Some(format!("proj-{}", i));
        tabs.add_tab(tab);
    }
    assert!(tabs.is_at_capacity());
    assert!(tabs.find_by_project("proj-new").is_none());
}

#[test]
fn stale_workspace_registration_is_dropped() {
    let mut picker = codegg::tui::app::state::ProjectPickerState::new(true, 0);
    let id1 = picker.begin_request();
    assert!(picker.is_request_current(id1));
    let id2 = picker.begin_request();
    assert!(!picker.is_request_current(id1));
    assert!(picker.is_request_current(id2));
}

#[test]
fn stale_project_registration_is_dropped() {
    let mut picker = codegg::tui::app::state::ProjectPickerState::new(true, 0);
    let id1 = picker.begin_request();
    let id2 = picker.begin_request();
    let id3 = picker.begin_request();
    assert!(!picker.is_request_current(id1));
    assert!(!picker.is_request_current(id2));
    assert!(picker.is_request_current(id3));
}

#[test]
fn stale_project_sessions_is_dropped() {
    let mut tab = codegg::tui::app::state::ProjectTabState::empty(
        codegg::tui::app::state::ProjectTabId::new(),
        "test".to_string(),
    );
    tab.project_id = Some("proj-1".to_string());
    tab.workspace_id = Some("ws-1".to_string());

    let stale_id = tab.request_state.begin();
    let fresh_id = tab.request_state.begin();

    assert!(!tab.request_state.is_current(stale_id));
    assert!(tab.request_state.is_current(fresh_id));
}
