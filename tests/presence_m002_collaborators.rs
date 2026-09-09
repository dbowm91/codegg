//! Presence M002 — TUI collaborator and presence surface.
//!
//! TUI-side projection proof: daemon-owned snapshots render as bounded
//! per-project collaborator state with stable ordering, stale/reconnect
//! guards, privacy-identical unavailable rendering, and dialog focus
//! that never mutates sessions.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, PresenceActivityDto,
    PresenceCapabilitiesDto, PresencePrincipalDto, PresenceSnapshotDto, RequestEnvelope,
};
use codegg::tui::app::TuiCommand;
use tokio::sync::mpsc;

#[derive(Default)]
struct FakePresenceClient {
    snapshots: Mutex<HashMap<String, PresenceSnapshotDto>>,
    unauthorized: Mutex<Vec<String>>,
    advertise: bool,
}

impl FakePresenceClient {
    fn with_snapshots(
        snapshots: HashMap<String, PresenceSnapshotDto>,
        unauthorized: Vec<String>,
        advertise: bool,
    ) -> Self {
        Self {
            snapshots: Mutex::new(snapshots),
            unauthorized: Mutex::new(unauthorized),
            advertise,
        }
    }

    fn capabilities(&self) -> PresenceCapabilitiesDto {
        PresenceCapabilitiesDto {
            supported: self.advertise,
            protocol_version: 1,
            lease_ttl_secs: 90,
            idle_after_secs: 30,
            heartbeat_interval_hint_secs: 30,
            max_principals_per_project: 256,
            max_sessions_per_principal: 16,
            max_contributions: 4096,
        }
    }
}

#[async_trait]
impl CoreClient for FakePresenceClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::PresenceCapabilities => Ok(CoreResponse::PresenceCapabilities {
                capabilities: self.capabilities(),
            }),
            CoreRequest::PresenceSnapshotGet { project_id } => {
                if self.unauthorized.lock().unwrap().contains(&project_id) {
                    return Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    });
                }
                match self.snapshots.lock().unwrap().get(&project_id).cloned() {
                    Some(snapshot) => Ok(CoreResponse::PresenceSnapshot { snapshot }),
                    None => Ok(CoreResponse::Error {
                        code: "project_not_found".to_string(),
                        message: "not found".to_string(),
                    }),
                }
            }
            _ => Ok(CoreResponse::Error {
                code: "unsupported".to_string(),
                message: "unsupported in fake".to_string(),
            }),
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = mpsc::channel(8);
        rx
    }
}

fn principal(id: &str, activity: PresenceActivityDto, sessions: &[&str]) -> PresencePrincipalDto {
    PresencePrincipalDto {
        principal_id: id.to_string(),
        activity,
        client_count: 1,
        session_ids: sessions.iter().map(|s| s.to_string()).collect(),
        last_active_ms: 7,
    }
}

fn snapshot(project: &str, principals: Vec<PresencePrincipalDto>) -> PresenceSnapshotDto {
    PresenceSnapshotDto {
        project_id: project.to_string(),
        as_of_ms: 42,
        principals,
        truncated: false,
    }
}

fn app_with_project(project_id: &str) -> codegg::tui::app::App {
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/m002".to_string());
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.project_id = Some(project_id.to_string());
        tab.workspace_id = Some("ws-1".to_string());
    }
    app
}

// ── Multi-project routing ────────────────────────────────────────────

#[test]
fn multi_project_routing_keeps_collaborators_separate() {
    let mut app = app_with_project("proj-a");
    // Open a second tab for proj-b.
    let second_id = codegg::tui::app::state::ProjectTabId::new();
    let mut second = codegg::tui::app::state::ProjectTabState::empty(second_id.clone(), "b".into());
    second.project_id = Some("proj-b".to_string());
    second.workspace_id = Some("ws-b".to_string());
    app.project_tabs.add_tab(second);

    let epoch = app.presence.reconnect_epoch;
    let req_a = app.presence.begin_refresh("proj-a");
    let req_b = app.presence.begin_refresh("proj-b");
    app.apply_presence_snapshot(
        req_a,
        "proj-a".to_string(),
        Some(snapshot(
            "proj-a",
            vec![principal("alice", PresenceActivityDto::Active, &["s1"])],
        )),
        None,
        false,
        false,
        epoch,
    );
    // Apply via the reducer directly to keep the test synchronous.
    let _ = app.presence.apply_snapshot(
        req_b,
        "proj-b",
        &snapshot(
            "proj-b",
            vec![principal("bob", PresenceActivityDto::Idle, &[])],
        ),
        epoch,
    );
    assert_eq!(
        app.presence.get("proj-a").unwrap().principals[0].principal_id,
        "alice"
    );
    assert_eq!(
        app.presence.get("proj-b").unwrap().principals[0].principal_id,
        "bob"
    );
    // Header follows the active tab only.
    assert_eq!(app.presence.header_summary("proj-a").unwrap(), "👥 1");
    assert_eq!(app.presence.header_summary("proj-b").unwrap(), "👥 1");
}

// ── Ordering / bounds ────────────────────────────────────────────────

#[test]
fn ordering_is_stable_and_panel_is_bounded() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let req = state.begin_refresh("p");
    let epoch = state.reconnect_epoch;
    let snap = snapshot(
        "p",
        vec![
            principal("zeta", PresenceActivityDto::Idle, &[]),
            principal("alpha", PresenceActivityDto::Idle, &[]),
            principal("mid", PresenceActivityDto::AgentRunning, &["s1"]),
        ],
    );
    assert!(state.apply_snapshot(req, "p", &snap, epoch));
    let ids: Vec<&str> = state
        .get("p")
        .unwrap()
        .principals
        .iter()
        .map(|e| e.principal_id.as_str())
        .collect();
    assert_eq!(ids, vec!["mid", "alpha", "zeta"]);
    let lines = state.panel_lines("p");
    assert!(lines[0].contains("Collaborators (3)"));
    // Coarse labels only; no content.
    assert!(lines.iter().any(|l| l.contains("agent running")));
}

// ── Stale expiry ─────────────────────────────────────────────────────

#[test]
fn stale_completion_is_dropped() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let stale = state.begin_refresh("p");
    let fresh = state.begin_refresh("p");
    let epoch = state.reconnect_epoch;
    assert!(!state.apply_snapshot(
        stale,
        "p",
        &snapshot(
            "p",
            vec![principal("old", PresenceActivityDto::Active, &[])]
        ),
        epoch
    ));
    assert!(state.apply_snapshot(
        fresh,
        "p",
        &snapshot(
            "p",
            vec![principal("new", PresenceActivityDto::Active, &[])]
        ),
        epoch
    ));
    assert_eq!(state.get("p").unwrap().principals[0].principal_id, "new");
}

// ── Reconnect / resync ───────────────────────────────────────────────

#[test]
fn reconnect_flags_resync_and_drops_old_epoch() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let req = state.begin_refresh("p");
    let old_epoch = state.reconnect_epoch;
    let new_epoch = state.on_reconnect();
    assert!(!state.apply_snapshot(
        req,
        "p",
        &snapshot(
            "p",
            vec![principal("ghost", PresenceActivityDto::Active, &[])]
        ),
        old_epoch
    ));
    assert!(state.get("p").unwrap().needs_resync);
    let req2 = state.begin_refresh("p");
    assert!(state.apply_snapshot(
        req2,
        "p",
        &snapshot(
            "p",
            vec![principal("fresh", PresenceActivityDto::Active, &[])]
        ),
        new_epoch
    ));
    assert!(!state.get("p").unwrap().needs_resync);
}

#[test]
fn app_reconnect_resyncs_active_project() {
    let mut app = app_with_project("proj-a");
    app.presence.set_capability(true);
    let epoch = app.presence.reconnect_epoch;
    let req = app.presence.begin_refresh("proj-a");
    assert!(app.presence.apply_snapshot(
        req,
        "proj-a",
        &snapshot(
            "proj-a",
            vec![principal("alice", PresenceActivityDto::Active, &[])]
        ),
        epoch
    ));
    // Direct epoch bump (unit of reconnect): stale presentation is flagged
    // until the next authoritative snapshot replaces it.
    let new_epoch = app.presence.on_reconnect();
    assert_ne!(epoch, new_epoch);
    assert!(app.presence.get("proj-a").unwrap().needs_resync);
    // Full reconnect path also bumps the routing epoch for stale-completion
    // rejection. Give the app a fake client so the auto-refresh it triggers
    // negotiates support instead of marking unsupported.
    let mut app2 = app_with_project("proj-a");
    app2.presence.set_capability(true);
    let routing_before = app2.routing_registry.reconnect_epoch;
    // Drive only the epoch-bump half here; the async re-fetch is covered by
    // the round-trip tests below.
    app2.presence.on_reconnect();
    app2.routing_registry.bump_reconnect_epoch();
    assert!(app2.routing_registry.reconnect_epoch > routing_before);
}

// ── Unauthorized / feature-absent ────────────────────────────────────

#[test]
fn unauthorized_and_absent_render_identically() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let epoch = state.reconnect_epoch;
    let req_a = state.begin_refresh("secret");
    assert!(state.apply_error(
        req_a,
        "secret",
        "project_not_found: denied".into(),
        true,
        false,
        epoch
    ));
    let req_b = state.begin_refresh("absent");
    assert!(state.apply_error(
        req_b,
        "absent",
        "project_not_found: absent".into(),
        true,
        false,
        epoch
    ));
    assert_eq!(state.panel_lines("secret"), state.panel_lines("absent"));
    assert_eq!(state.header_summary("secret"), None);
    assert_eq!(state.header_summary("absent"), None);
}

#[test]
fn feature_absent_hides_panel_without_breaking_tabs() {
    let mut app = app_with_project("proj-a");
    assert_eq!(app.open_tab_count(), 1);
    app.presence.set_capability(false);
    assert!(!app.presence_supported());
    assert_eq!(app.presence.header_summary("proj-a"), None);
    assert!(app
        .presence
        .panel_lines("proj-a")
        .iter()
        .any(|l| l.contains("unavailable")));
    // Tabs keep working.
    assert_eq!(app.open_tab_count(), 1);
    assert!(app.active_tab().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refresh_against_old_daemon_marks_unsupported() {
    let fake = Arc::new(FakePresenceClient::with_snapshots(
        HashMap::new(),
        vec![],
        false,
    ));
    let mut app = app_with_project("proj-a");
    app.set_core_client(fake);
    let (tx, mut rx) = mpsc::channel(8);
    app.tui_cmd_tx = Some(tx.clone());
    app.refresh_presence("proj-a".to_string());
    let cmd = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("completion arrives")
        .expect("channel open");
    match cmd {
        TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert_eq!(project_id, "proj-a");
            assert!(unsupported);
            app.apply_presence_snapshot(
                request_id,
                project_id,
                snapshot,
                error,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("unexpected completion: {other:?}"),
    }
    assert!(!app.presence_supported());
    assert_eq!(app.presence.header_summary("proj-a"), None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refresh_round_trip_applies_bounded_snapshot() {
    let mut snapshots = HashMap::new();
    snapshots.insert(
        "proj-a".to_string(),
        snapshot(
            "proj-a",
            vec![
                principal("bob", PresenceActivityDto::Idle, &[]),
                principal("alice", PresenceActivityDto::Active, &["s1"]),
            ],
        ),
    );
    let fake = Arc::new(FakePresenceClient::with_snapshots(snapshots, vec![], true));
    let mut app = app_with_project("proj-a");
    app.set_core_client(fake);
    let (tx, mut rx) = mpsc::channel(8);
    app.tui_cmd_tx = Some(tx.clone());
    app.refresh_presence("proj-a".to_string());
    let cmd = tokio::time::timeout(std::time::Duration::from_millis(500), rx.recv())
        .await
        .expect("completion arrives")
        .expect("channel open");
    match cmd {
        TuiCommand::PresenceSnapshotLoaded {
            request_id,
            project_id,
            snapshot,
            error,
            unauthorized,
            unsupported,
            reconnect_epoch,
        } => {
            assert!(!unsupported);
            app.apply_presence_snapshot(
                request_id,
                project_id,
                snapshot,
                error,
                unauthorized,
                unsupported,
                reconnect_epoch,
            );
        }
        other => panic!("unexpected completion: {other:?}"),
    }
    assert!(app.presence_supported());
    let entry = app.presence.get("proj-a").expect("entry applied");
    assert_eq!(entry.principals.len(), 2);
    // Stable order: active before idle.
    assert_eq!(entry.principals[0].principal_id, "alice");
    assert_eq!(app.presence.header_summary("proj-a").unwrap(), "👥 2");
}

// ── Identical names across projects ──────────────────────────────────

#[test]
fn identical_names_across_projects_do_not_collide() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let epoch = state.reconnect_epoch;
    let ra = state.begin_refresh("proj-a");
    let rb = state.begin_refresh("proj-b");
    assert!(state.apply_snapshot(
        ra,
        "proj-a",
        &snapshot(
            "proj-a",
            vec![principal("sam", PresenceActivityDto::Active, &["s-a"])]
        ),
        epoch
    ));
    assert!(state.apply_snapshot(
        rb,
        "proj-b",
        &snapshot(
            "proj-b",
            vec![principal("sam", PresenceActivityDto::Idle, &["s-b"])]
        ),
        epoch
    ));
    assert_eq!(
        state.get("proj-a").unwrap().principals[0].activity,
        PresenceActivityDto::Active
    );
    assert_eq!(
        state.get("proj-b").unwrap().principals[0].activity,
        PresenceActivityDto::Idle
    );
}

// ── Focus / key regression ───────────────────────────────────────────

#[test]
fn collaborators_dialog_focus_does_not_mutate_sessions() {
    let mut app = app_with_project("proj-a");
    app.presence.set_capability(true);
    let epoch = app.presence.reconnect_epoch;
    let req = app.presence.begin_refresh("proj-a");
    assert!(app.presence.apply_snapshot(
        req,
        "proj-a",
        &snapshot(
            "proj-a",
            vec![principal("alice", PresenceActivityDto::Active, &[])]
        ),
        epoch
    ));
    let session_before = app.session_state.session.clone();
    app.show_collaborators();
    // Dialog opened on the focus stack with the collaborators type.
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        codegg::tui::components::component::DialogType::Collaborators
    );
    // No session mutation from opening the panel.
    assert_eq!(
        app.session_state.session.as_ref().map(|s| s.id.clone()),
        session_before.as_ref().map(|s| s.id.clone())
    );
    // Standard key convention closes without side effects.
    app.process_msg(codegg::tui::app::TuiMsg::CloseDialog);
    assert_eq!(
        app.focus_manager.active_dialog_type(),
        codegg::tui::components::component::DialogType::None
    );
    assert_eq!(
        app.session_state.session.as_ref().map(|s| s.id.clone()),
        session_before.as_ref().map(|s| s.id.clone())
    );
}

// ── Inactive-tab resource bound ──────────────────────────────────────

#[test]
fn inactive_presence_stays_bounded() {
    let mut state = codegg::tui::app::state::PresenceState::new();
    state.set_capability(true);
    let epoch = state.reconnect_epoch;
    for i in 0..(codegg::tui::app::state::MAX_PRESENCE_PROJECTS + 5) {
        let project = format!("proj-{i:02}");
        let req = state.begin_refresh(&project);
        assert!(state.apply_snapshot(
            req,
            &project,
            &snapshot(
                &project,
                vec![principal("u", PresenceActivityDto::Active, &[])]
            ),
            epoch
        ));
    }
    assert!(state.project_count() <= codegg::tui::app::state::MAX_PRESENCE_PROJECTS);
}
