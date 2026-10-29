//! M013 trajectory: LSP **read delivery** correctness (ADR-0012 §2 and §3).
//!
//! M013 exists because M006-B shipped a read surface whose delivery was
//! wrong in ways no unit test could see. Two of them are proven here, and a
//! test for each exists because both were invisible before:
//!
//! 1. **The publisher had no caller.** `publish_changed_diagnostics` was
//!    correct and had zero callers, so `LspDiagnosticsUpdated` was never
//!    emitted, the resync authority had nothing to be authoritative about,
//!    and every client read the stream as permanently cold. Proven by
//!    [`a_warm_sessions_diagnostics_reach_the_stream_without_a_frontend_asking`].
//! 2. **"Several warm servers" was reported as "no warm server".** The
//!    resolver's every error was mapped to `NotReady`, so a polyglot
//!    workspace with two healthy servers was indistinguishable from a cold
//!    one. Proven by
//!    [`several_warm_servers_are_ambiguous_not_not_ready`].
//!
//! A third property is load-bearing enough to pin here even though it was
//! not a separate defect: a read against a cold root must start no process.
//! That is ADR-0012 §3, and after M013 it is a property of the call graph
//! rather than of a check someone remembered to write first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::lsp::service::LspService;
use codegg_client::{DocumentController, DocumentTransport};
use codegg_core::identity::ProjectId;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use codegg_core::transport_auth::PersonalTokenStore;
use codegg_protocol::core::{CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope};
use codegg_protocol::document::DocumentScope;
use codegg_protocol::lsp::{
    LspControlActionDto, LspControlRequestDto, LspDiagnosticsGetRequestDto, LspDiagnosticsStatus,
    LspDiagnosticsSubscribeRequestDto, LspReadOperation, LspReadPayloadDto, LspReadRequestDto,
    LspReadResultDto, LspReadStatus, LspServerStatusGetRequestDto, LspWorkflowKindDto,
    LspWorkflowRequestDto,
};
use egglsp::{LspConfig, LspRule};
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::mpsc;

const M002_LATENCY_SAMPLE_COUNT: usize = 31;

const MEMBER: &str = "client-member";
const PEER: &str = "client-peer";
const MAINTAINER: &str = "client-maintainer";
const SERVER: &str = "rust-analyzer";
const OTHER_SERVER: &str = "gopls";

fn fake_server() -> PathBuf {
    option_env!("CARGO_BIN_EXE_codegg-lsp-test-server")
        .map(PathBuf::from)
        .expect("build with lsp-test-support")
}

fn init_step() -> Value {
    json!({
        "type": "ExpectRequest",
        "method": "initialize",
        "then": [{"type": "RespondResult", "result": {"capabilities": {"hoverProvider": true, "workspaceSymbolProvider": true}}}]
    })
}

fn initialized_step(then: Vec<Value>) -> Value {
    json!({"type": "ExpectNotification", "method": "initialized", "then": then})
}

/// Push one diagnostic for `uri`, so the collector has something for the
/// publisher to find.
///
/// The URI is literal: `__SOURCE_URI__` is a token of the *production* test
/// harness, not of this scenario engine, and a scenario that leaves it
/// unsubstituted sends a notification for a file that does not exist.
fn push_diagnostic_step(uri: &str) -> Value {
    json!({
        "type": "SendNotification",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "version": 1,
            "diagnostics": [{
                "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 3}},
                "severity": 1,
                "message": "M013 synthetic diagnostic",
                "code": "M013"
            }]
        }
    })
}

fn hover_step(result: Value) -> Value {
    json!({
        "type": "ExpectRequest",
        "method": "textDocument/hover",
        "then": [{"type": "RespondResult", "result": result}]
    })
}

/// A scenario that only cares that the server came up and answered. Not
/// strict: these tests assert on the **daemon's** resolution, and an
/// unexpected message ordering would fail a strict script for a reason that
/// has nothing to do with the property under test.
fn scenario(name: &str, steps: Vec<Value>) -> Value {
    json!({
        "name": name,
        "steps": steps,
        "exit": {"type": "ExitCode", "code": 0},
        "strict": false
    })
}

fn rule_for(root: &Path, id: &str, extension: &str, steps: Vec<Value>) -> LspRule {
    let scenario_path = root.join(format!("{id}-scenario.json"));
    let transcript = root.join(format!("{id}-transcript.jsonl"));
    std::fs::write(
        &scenario_path,
        serde_json::to_vec(&scenario(id, steps)).expect("serialize scenario"),
    )
    .expect("write scenario");
    let env = HashMap::from([
        (
            "CODEGG_FAKE_LSP_SCENARIO".into(),
            scenario_path.display().to_string(),
        ),
        (
            "CODEGG_FAKE_LSP_TRANSCRIPT".into(),
            transcript.display().to_string(),
        ),
    ]);
    LspRule::Active {
        command: vec![fake_server().display().to_string()],
        extensions: Some(vec![extension.into()]),
        disabled: None,
        env: Some(env),
        initialization: None,
        workspace_configuration: None,
        restart: None,
    }
}

/// A service with `rust-analyzer` answering `.rs` and `gopls` answering
/// `.go`, both rooted at the same workspace.
///
/// Two roots would make the polyglot case unrepresentable; one root with two
/// claims is the case that actually happens.
fn service_for(root: &Path, rust_steps: Vec<Value>, go_steps: Vec<Value>) -> Arc<LspService> {
    LspService::new_arc(LspConfig::Rules(HashMap::from([
        (SERVER.into(), rule_for(root, SERVER, "rs", rust_steps)),
        (
            OTHER_SERVER.into(),
            rule_for(root, OTHER_SERVER, "go", go_steps),
        ),
    ])))
}

struct Fixture {
    _temp: TempDir,
    root: PathBuf,
    workspace_id: String,
    service: Arc<LspService>,
    daemon: Arc<CoreDaemon>,
    project_id: String,
    session_id: String,
}

impl Fixture {
    /// Open the Rust document through the daemon.
    ///
    /// This does two things the tests need and neither is incidental: it is
    /// what binds the calling client to the project scope and pushes the
    /// document to the configured server.
    async fn open_lib(&self) {
        match call(
            &self.daemon,
            MEMBER,
            CoreRequest::DocumentOpen {
                project_id: self.project_id.clone(),
                workspace_id: self.workspace_id.clone(),
                relative_path: "src/lib.rs".to_string(),
                scope: DocumentScope::Workspace,
            },
        )
        .await
        {
            CoreResponse::DocumentSnapshot { .. } => {}
            other => panic!("document open failed: {other:?}"),
        }
    }
}

#[derive(Clone)]
struct TuiDaemonAdapter {
    daemon: Arc<CoreDaemon>,
    client_id: String,
}

#[async_trait]
impl CoreClient for TuiDaemonAdapter {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        self.daemon
            .handle_request_for_client(request, &self.client_id)
            .await
    }

    fn subscribe(&self) -> mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (tx, rx) = mpsc::channel(256);
        let mut events = self.daemon.event_log.subscribe();
        tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        rx
    }
}

#[async_trait]
impl DocumentTransport for TuiDaemonAdapter {
    async fn request(&self, request: CoreRequest) -> Result<CoreResponse, String> {
        self.daemon
            .handle_request_for_client(
                new_request(format!("req-tui-doc-{}", uuid::Uuid::new_v4()), request),
                &self.client_id,
            )
            .await
            .map_err(|error| error.to_string())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_tui_hover_uses_the_live_daemon_request_coordinator() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;

    let adapter = Arc::new(TuiDaemonAdapter {
        daemon: fixture.daemon.clone(),
        client_id: MEMBER.to_owned(),
    });
    let controller = Arc::new(DocumentController::new(adapter.clone()));
    controller
        .open(
            fixture.project_id.clone(),
            fixture.workspace_id.clone(),
            "src/lib.rs".to_owned(),
            false,
            DocumentScope::Workspace,
        )
        .await
        .expect("TUI document attachment uses the authorized live daemon client");

    let mut app = codegg::tui::app::App::new_for_testing(fixture.root.display().to_string());
    let tab = app.active_tab_mut().expect("default project tab");
    tab.project_id = Some(fixture.project_id.clone());
    tab.workspace_id = Some(fixture.workspace_id.clone());
    tab.workspace_root = Some(fixture.root.clone());
    tab.session_id = Some(fixture.session_id.clone());
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Editor);
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    let mut session =
        codegg::tui::document_session::TuiDocumentSession::from_controller(controller);
    session.presentation_mut().mode = codegg::tui::editor::EditorMode::Normal;
    app.editor_state.session = Some(session);
    app.editor_state.path = Some("src/lib.rs".to_owned());
    app.editor_state.status = codegg::tui::app::state::editor::EditorStatus::Synced;
    app.set_core_client(adapter);
    let (tx, mut rx) = mpsc::channel(4);
    app.tui_cmd_tx = Some(tx);

    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    ));
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('k'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let completion = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("the asynchronous TUI hover returns")
        .expect("TUI command channel remains open");
    match completion {
        codegg::tui::app::TuiCommand::EditorLspReadFinished {
            operation,
            result: Some(result),
            error: None,
            ..
        } => {
            assert_eq!(operation, LspReadOperation::Hover);
            assert_eq!(result.status, LspReadStatus::Ready);
            assert!(matches!(
                result.payload,
                Some(LspReadPayloadDto::Hover { text, .. }) if text.contains("from rust")
            ));
        }
        other => panic!("expected a successful coordinated hover, got {other:?}"),
    }
    assert_eq!(fixture.service.client_keys().await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn production_tui_diagnostics_uses_the_authorized_live_subscription() {
    let fixture = fixture(RustBehaviour::Diagnostics, go_only_warming()).await;
    fixture.open_lib().await;
    fixture.daemon.publish_diagnostics_once().await;

    let adapter = Arc::new(TuiDaemonAdapter {
        daemon: fixture.daemon.clone(),
        client_id: MEMBER.to_owned(),
    });
    let controller = Arc::new(DocumentController::new(adapter.clone()));
    controller
        .open(
            fixture.project_id.clone(),
            fixture.workspace_id.clone(),
            "src/lib.rs".to_owned(),
            false,
            DocumentScope::Workspace,
        )
        .await
        .expect("TUI document attachment uses the authorized live daemon client");

    let mut app = codegg::tui::app::App::new_for_testing(fixture.root.display().to_string());
    let tab = app.active_tab_mut().expect("default project tab");
    tab.project_id = Some(fixture.project_id.clone());
    tab.workspace_id = Some(fixture.workspace_id.clone());
    tab.workspace_root = Some(fixture.root.clone());
    tab.session_id = Some(fixture.session_id.clone());
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Editor);
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    let mut session =
        codegg::tui::document_session::TuiDocumentSession::from_controller(controller);
    session.presentation_mut().mode = codegg::tui::editor::EditorMode::Normal;
    app.editor_state.session = Some(session);
    app.editor_state.path = Some("src/lib.rs".to_owned());
    app.editor_state.status = codegg::tui::app::state::editor::EditorStatus::Synced;
    app.set_core_client(adapter);
    let (tx, mut rx) = mpsc::channel(4);
    app.tui_cmd_tx = Some(tx);

    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    ));
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('d'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let completion = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("the asynchronous TUI diagnostics pull returns")
        .expect("TUI command channel remains open");
    match completion {
        codegg::tui::app::TuiCommand::EditorDiagnosticsFinished {
            result: Some(result),
            error: None,
            ..
        } => {
            assert_eq!(result.status, LspDiagnosticsStatus::Ready);
            assert_eq!(result.files.len(), 1);
            assert!(!result.files[0].diagnostics.is_empty());
        }
        other => panic!("expected the production diagnostics subscription result, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_authorized_clients_keep_independent_diagnostics_subscriptions() {
    let fixture = fixture(RustBehaviour::MultiClientParity, go_only_warming()).await;
    fixture.open_lib().await;
    fixture.daemon.publish_diagnostics_once().await;

    async fn subscribe(fixture: &Fixture, client_id: &str) -> CoreResponse {
        call(
            &fixture.daemon,
            client_id,
            CoreRequest::LspDiagnosticsSubscribe {
                request: LspDiagnosticsSubscribeRequestDto {
                    project_id: fixture.project_id.clone(),
                    cursor: None,
                    projection_version: codegg_protocol::projection::PROJECTION_PROTOCOL_VERSION,
                },
            },
        )
        .await
    }

    let first = subscribe(&fixture, MEMBER).await;
    let peer = subscribe(&fixture, PEER).await;
    let (first_id, first_snapshot) = match first {
        CoreResponse::ProjectionSubscribed {
            subscription_id,
            snapshot:
                codegg_protocol::projection::replay::ProjectionSnapshotBundle::LspDiagnostics {
                    files,
                    ..
                },
            ..
        } => (subscription_id, files),
        other => panic!("first authorized diagnostics subscription failed: {other:?}"),
    };
    let (peer_id, peer_snapshot) = match peer {
        CoreResponse::ProjectionSubscribed {
            subscription_id,
            snapshot:
                codegg_protocol::projection::replay::ProjectionSnapshotBundle::LspDiagnostics {
                    files,
                    ..
                },
            ..
        } => (subscription_id, files),
        other => panic!("second authorized diagnostics subscription failed: {other:?}"),
    };
    assert_ne!(
        first_id, peer_id,
        "each client owns a distinct subscription id"
    );
    assert_eq!(first_snapshot, peer_snapshot);
    assert_eq!(first_snapshot.len(), 1);
    assert!(!first_snapshot[0].diagnostics.is_empty());

    let tui_adapter = Arc::new(TuiDaemonAdapter {
        daemon: fixture.daemon.clone(),
        client_id: MEMBER.to_owned(),
    });
    let controller = Arc::new(DocumentController::new(tui_adapter.clone()));
    controller
        .open(
            fixture.project_id.clone(),
            fixture.workspace_id.clone(),
            "src/lib.rs".to_owned(),
            false,
            DocumentScope::Workspace,
        )
        .await
        .expect("TUI attaches through its authorized daemon client");
    let mut app = codegg::tui::app::App::new_for_testing(fixture.root.display().to_string());
    let tab = app.active_tab_mut().expect("default project tab");
    tab.project_id = Some(fixture.project_id.clone());
    tab.workspace_id = Some(fixture.workspace_id.clone());
    tab.workspace_root = Some(fixture.root.clone());
    tab.session_id = Some(fixture.session_id.clone());
    app.ui_state
        .routes
        .navigate_to(codegg::tui::route::Route::Editor);
    app.ui_state.input_mode = codegg::tui::input::InputMode::Normal;
    let mut session =
        codegg::tui::document_session::TuiDocumentSession::from_controller(controller);
    session.presentation_mut().mode = codegg::tui::editor::EditorMode::Normal;
    app.editor_state.session = Some(session);
    app.editor_state.path = Some("src/lib.rs".to_owned());
    app.editor_state.status = codegg::tui::app::state::editor::EditorStatus::Synced;
    app.set_core_client(tui_adapter);
    let (tx, mut tui_rx) = mpsc::channel(4);
    app.tui_cmd_tx = Some(tx);
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char(' '),
        crossterm::event::KeyModifiers::NONE,
    ));
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('k'),
        crossterm::event::KeyModifiers::NONE,
    ));

    let peer_coordinator = codegg_client::LspRequestCoordinator::default();
    let peer_scope = codegg_client::LspRequestScope {
        frontend_id: "peer-frontend".into(),
        attachment_id: fixture.session_id.clone(),
        route_generation: 1,
        connection_generation: 1,
        project_id: fixture.project_id.clone(),
        workspace_id: fixture.workspace_id.clone(),
        session_id: fixture.session_id.clone(),
        document_id: None,
        path: Some("src/lib.rs".into()),
        server_id: Some(SERVER.into()),
    };
    let peer_ticket = match peer_coordinator.begin(codegg_client::LspRequestKey {
        scope: peer_scope.clone(),
        operation: codegg_client::LspRequestOperation::Read(LspReadOperation::Hover),
        document_generation: 1,
        position: Some((0, 8)),
        argument: None,
    }) {
        codegg_client::LspRequestStart::Started(ticket) => ticket,
        other => panic!("peer frontend request was not admitted: {other:?}"),
    };
    let peer_daemon = fixture.daemon.clone();
    let peer_request = read_request(&fixture, Some(SERVER));
    let (tui_completion, peer_response) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(5), tui_rx.recv()),
        call(
            &peer_daemon,
            PEER,
            CoreRequest::LspReadGet {
                request: peer_request,
            },
        ),
    );
    match tui_completion
        .expect("production TUI hover completes")
        .expect("TUI command channel remains open")
    {
        codegg::tui::app::TuiCommand::EditorLspReadFinished {
            operation,
            result: Some(result),
            error: None,
            ..
        } => {
            assert_eq!(operation, LspReadOperation::Hover);
            assert_eq!(result.status, LspReadStatus::Ready);
        }
        other => panic!("expected the production TUI hover result, got {other:?}"),
    }
    let peer_result = as_read(peer_response);
    assert_eq!(peer_result.status, LspReadStatus::Ready);
    assert!(peer_coordinator.finish(&peer_ticket, &peer_scope, 1));
    let server_keys = fixture.service.client_keys().await;
    assert_eq!(
        server_keys.len(),
        1,
        "both frontend clients share one warm LSP process for the same root/server/profile: {server_keys:?}"
    );

    let removed = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::ProjectionUnsubscribe {
            subscription_id: first_id,
        },
    )
    .await;
    assert!(matches!(
        removed,
        CoreResponse::ProjectionUnsubscribed { .. }
    ));
    let peer_removed = call(
        &fixture.daemon,
        PEER,
        CoreRequest::ProjectionUnsubscribe {
            subscription_id: peer_id,
        },
    )
    .await;
    assert!(matches!(
        peer_removed,
        CoreResponse::ProjectionUnsubscribed { .. }
    ));
}

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn human_token(team: &TeamStore, name: &str, client_id: &str) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap()
}

async fn local_call(daemon: &CoreDaemon, name: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request(new_request(format!("req-{name}"), request)))
        .await
        .unwrap()
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{client}-{}", uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

/// A workspace holding one Rust and one Go source file, a daemon bound to two
/// fake servers, and a member client entitled to read.
async fn fixture(behaviour: RustBehaviour, go_steps: Vec<Value>) -> Fixture {
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    // Both manifests at one root: `find_project_root` then resolves a `.rs`
    // and a `.go` file to the *same* project root, which is what puts two
    // warm clients under one root key prefix.
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='m013'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("go.mod"), "module m013\n\ngo 1.21\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn alpha() -> u32 { 1 }\n").unwrap();
    std::fs::write(root.join("src/main.go"), "package main\n\nfunc main() {}\n").unwrap();

    let lib_uri = format!("file://{}", root.join("src/lib.rs").display());
    let service = service_for(&root, behaviour.steps(&lib_uri), go_steps);
    let pool = test_pool().await;
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool.clone()), None, None).with_lsp_service(service.clone()),
    );

    let workspace_id = match local_call(
        &daemon,
        "ws",
        CoreRequest::WorkspaceRegister {
            root: root.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match local_call(
        &daemon,
        "proj",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "m013".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, ?, 1, 1, '[]')",
    )
    .bind(&project_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO session (id, project_id, workspace_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, 's', ?, 't', 'v', 1, 1)",
    )
    .bind(&session_id)
    .bind(&project_id)
    .bind(&workspace_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();

    let team = TeamStore::new(pool.clone());
    let member = human_token(&team, "Member", MEMBER).await;
    let principal = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    daemon.clients.register_with_principal(
        MEMBER.to_owned(),
        format!("{MEMBER}-name"),
        None,
        member,
    );
    let peer = human_token(&team, "Peer", PEER).await;
    let peer_principal = team
        .get_principal(peer.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &peer_principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    daemon
        .clients
        .register_with_principal(PEER.to_owned(), format!("{PEER}-name"), None, peer);
    let maintainer = human_token(&team, "Maintainer", MAINTAINER).await;
    let maintainer_principal = team
        .get_principal(maintainer.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &maintainer_principal.id,
        ProjectRole::Maintainer,
    )
    .await
    .unwrap();
    daemon.clients.register_with_principal(
        MAINTAINER.to_owned(),
        format!("{MAINTAINER}-name"),
        None,
        maintainer,
    );
    let peer = human_token(&team, "Peer", PEER).await;
    let peer_principal = team
        .get_principal(peer.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &ProjectId::parse(&project_id).expect("registered project id parses"),
        &peer_principal.id,
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    daemon
        .clients
        .register_with_principal(PEER.to_owned(), format!("{PEER}-name"), None, peer);
    // A live session, as the idle wake and the diagnostics cadence both see
    // it. The cadence enumerates this registry, not the session table.
    daemon
        .bind_runtime_for_session(&session_id)
        .await
        .expect("bind runtime for session");

    Fixture {
        _temp: temp,
        root,
        workspace_id,
        service,
        daemon: Arc::new(daemon),
        project_id,
        session_id,
    }
}

/// What the Rust server does once it is up.
#[derive(Clone, Copy)]
enum RustBehaviour {
    /// Push one diagnostic, so the publisher has something to publish.
    Diagnostics,
    MultiClientParity,
    /// Answer a hover, so a point read has something to return.
    Hover,
    Workflow,
    /// Answer enough hovers for the bounded M002 latency sample.
    LatencySample,
    SelectionRange,
    InvalidSelectionRange,
    DeepSelectionRange,
    OversizedSelectionRange,
}

impl RustBehaviour {
    fn steps(self, lib_uri: &str) -> Vec<Value> {
        let init = match self {
            RustBehaviour::Workflow => json!({
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {
                    "documentSymbolProvider": true
                }}}]
            }),
            RustBehaviour::SelectionRange
            | RustBehaviour::InvalidSelectionRange
            | RustBehaviour::DeepSelectionRange
            | RustBehaviour::OversizedSelectionRange => json!({
                "type": "ExpectRequest",
                "method": "initialize",
                "then": [{"type": "RespondResult", "result": {"capabilities": {
                    "hoverProvider": true,
                    "workspaceSymbolProvider": true,
                    "selectionRangeProvider": true
                }}}]
            }),
            _ => init_step(),
        };
        let initialized_actions = match self {
            RustBehaviour::Diagnostics | RustBehaviour::MultiClientParity => {
                vec![push_diagnostic_step(lib_uri)]
            }
            _ => Vec::new(),
        };
        let mut steps = vec![init, initialized_step(initialized_actions)];
        match self {
            RustBehaviour::SelectionRange => steps.push(json!({
                "type": "ExpectRequest",
                "method": "textDocument/selectionRange",
                "then": [{
                    "type": "RespondResult",
                    "result": [{
                        "range": {
                            "start": {"line": 0, "character": 7},
                            "end": {"line": 0, "character": 12}
                        },
                        "parent": {
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 29}
                            }
                        }
                    }]
                }]
            })),
            RustBehaviour::Workflow => {
                for _ in 0..2 {
                    steps.push(json!({
                        "type": "ExpectRequest",
                        "method": "textDocument/documentSymbol",
                        "then": [{"type": "RespondResult", "result": []}]
                    }));
                }
            }
            RustBehaviour::InvalidSelectionRange => steps.push(json!({
                "type": "ExpectRequest",
                "method": "textDocument/selectionRange",
                "then": [{
                    "type": "RespondResult",
                    "result": [{
                        "range": {
                            "start": {"line": 0, "character": 12},
                            "end": {"line": 0, "character": 7}
                        }
                    }]
                }]
            })),
            RustBehaviour::DeepSelectionRange => {
                let range = json!({
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 29}
                });
                let mut chain = json!({"range": range});
                for _ in 0..codegg_protocol::lsp::MAX_LSP_SELECTION_RANGE_DEPTH {
                    chain = json!({"range": range, "parent": chain});
                }
                steps.push(json!({
                    "type": "ExpectRequest",
                    "method": "textDocument/selectionRange",
                    "then": [{"type": "RespondResult", "result": [chain]}]
                }));
            }
            RustBehaviour::OversizedSelectionRange => {
                let padding =
                    "x".repeat(codegg_protocol::lsp::MAX_LSP_SELECTION_RANGE_RESPONSE_BYTES + 1);
                steps.push(json!({
                    "type": "ExpectRequest",
                    "method": "textDocument/selectionRange",
                    "then": [{
                        "type": "RespondResult",
                        "result": [{
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 29}
                            },
                            "padding": padding
                        }]
                    }]
                }));
            }
            _ => {}
        }
        let hover_count = match self {
            RustBehaviour::Hover => 1,
            RustBehaviour::LatencySample => 160,
            RustBehaviour::Diagnostics => 0,
            RustBehaviour::MultiClientParity => 2,
            RustBehaviour::SelectionRange
            | RustBehaviour::Workflow
            | RustBehaviour::InvalidSelectionRange
            | RustBehaviour::DeepSelectionRange
            | RustBehaviour::OversizedSelectionRange => 0,
        };
        for _ in 0..hover_count {
            steps.push(hover_step(json!({
                "contents": {"kind": "markdown", "value": "from rust"}
            })));
        }
        steps
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_read_is_warm_bounded_and_returns_only_ranges() {
    let fixture = fixture(RustBehaviour::SelectionRange, go_only_warming()).await;
    fixture.open_lib().await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let result = as_read(call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await);

    assert_eq!(result.status, LspReadStatus::Ready);
    match result.payload {
        Some(LspReadPayloadDto::SelectionRanges { ranges, truncated }) => {
            assert!(!truncated);
            assert_eq!(ranges.len(), 2);
            assert_eq!(ranges[0].start_column, 7);
            assert_eq!(ranges[0].end_column, 12);
            assert_eq!(ranges[1].start_column, 0);
            assert_eq!(ranges[1].end_column, 29);
            assert!(ranges
                .iter()
                .all(|range| range.workspace_relative_path.as_deref() == Some("src/lib.rs")));
        }
        other => panic!("expected bounded selection ranges, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_reports_unsupported_without_an_approximation() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let result = as_read(call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await);

    assert_eq!(result.status, LspReadStatus::Unsupported);
    assert!(matches!(
        result.payload,
        Some(LspReadPayloadDto::UnsupportedCapability { ref capability })
            if capability == "selectionRangeProvider"
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_rejects_a_reversed_server_range() {
    let fixture = fixture(RustBehaviour::InvalidSelectionRange, go_only_warming()).await;
    fixture.open_lib().await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let response = call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await;

    match response {
        CoreResponse::Error { code, .. } => assert_eq!(code, "lsp_server_failed"),
        other => panic!("expected an invalid server range refusal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_caps_depth_and_reports_truncation() {
    let fixture = fixture(RustBehaviour::DeepSelectionRange, go_only_warming()).await;
    fixture.open_lib().await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let result = as_read(call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await);

    match result.payload {
        Some(LspReadPayloadDto::SelectionRanges { ranges, truncated }) => {
            assert_eq!(
                ranges.len(),
                codegg_protocol::lsp::MAX_LSP_SELECTION_RANGE_DEPTH
            );
            assert!(truncated);
        }
        other => panic!("expected capped selection ranges, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_rejects_an_oversized_server_response() {
    let fixture = fixture(RustBehaviour::OversizedSelectionRange, go_only_warming()).await;
    fixture.open_lib().await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let response = call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await;

    match response {
        CoreResponse::Error { code, .. } => assert_eq!(code, "lsp_server_failed"),
        other => panic!("expected an oversized response refusal, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn selection_range_on_a_cold_root_does_not_start_a_server() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    let mut request = read_request(&fixture, Some(SERVER));
    request.operation = LspReadOperation::SelectionRange;

    let result = as_read(call(&fixture.daemon, MEMBER, CoreRequest::LspReadGet { request }).await);

    assert_eq!(result.status, LspReadStatus::NotReady);
    assert!(fixture.service.client_keys().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_status_is_session_scoped_and_control_needs_maintainer_capability() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;

    let status = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspServerStatusGet {
            request: LspServerStatusGetRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: None,
            },
        },
    )
    .await;
    match status {
        CoreResponse::LspServerStatusGetResult { result } => {
            assert!(!result.server_not_found);
            assert_eq!(result.servers.len(), 1);
            assert_eq!(result.servers[0].server_id, SERVER);
            assert_eq!(result.servers[0].stderr_tail, Vec::<String>::new());
            assert!(result.servers[0]
                .capabilities
                .iter()
                .any(|capability| capability == "hoverProvider"));
        }
        other => panic!("expected the session's attached server status, got {other:?}"),
    }

    let peer_status = call(
        &fixture.daemon,
        PEER,
        CoreRequest::LspServerStatusGet {
            request: LspServerStatusGetRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: None,
            },
        },
    )
    .await;
    match peer_status {
        CoreResponse::LspServerStatusGetResult { result } => {
            assert_eq!(result.servers.len(), 1);
            assert_eq!(result.servers[0].server_id, SERVER);
        }
        other => panic!("the peer must inspect the same daemon server: {other:?}"),
    }
    let canonical_root = fixture
        .root
        .canonicalize()
        .expect("canonical workspace root");
    assert_eq!(
        fixture
            .service
            .existing_client_keys_for_root(&canonical_root)
            .await
            .len(),
        1,
        "two clients inspecting the root must share one attached server"
    );

    let denied = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: SERVER.to_owned(),
                action: LspControlActionDto::Restart,
            },
        },
    )
    .await;
    match denied {
        CoreResponse::Error { code, .. } => assert_eq!(code, "authorization_denied"),
        other => panic!("a Contributor must not control the shared LSP, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn maintainer_control_stops_only_the_selected_warm_server_and_scopes_cache() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    let canonical_root = fixture
        .root
        .canonicalize()
        .expect("canonical workspace root");
    fixture.open_lib().await;
    fixture
        .service
        .get_or_create_client(&fixture.root.join("src/main.go"))
        .await
        .expect("warm the second language server");
    assert_eq!(
        fixture
            .service
            .existing_client_keys_for_root(&canonical_root)
            .await
            .len(),
        2
    );

    let denied = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: SERVER.to_owned(),
                action: LspControlActionDto::Stop,
            },
        },
    )
    .await;
    assert!(
        matches!(
            &denied,
            CoreResponse::Error { code, .. } if code == "authorization_denied"
        ),
        "Contributor stop must fail at the capability gate: {denied:?}"
    );

    let stopped = call(
        &fixture.daemon,
        MAINTAINER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: SERVER.to_owned(),
                action: LspControlActionDto::Stop,
            },
        },
    )
    .await;
    match stopped {
        CoreResponse::LspControlResult { result } => {
            assert_eq!(result.server_id, SERVER);
            assert_eq!(result.state, "stopped");
        }
        other => panic!("maintainer stop should complete through LSP control: {other:?}"),
    }
    assert_eq!(
        fixture
            .service
            .existing_client_keys_for_root(&canonical_root)
            .await,
        vec![format!("{}:{OTHER_SERVER}", canonical_root.display())],
        "stopping one server must leave the other session-workspace client attached"
    );

    let cache_status = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: String::new(),
                action: LspControlActionDto::CacheStatus,
            },
        },
    )
    .await;
    match cache_status {
        CoreResponse::LspControlResult { result } => {
            assert_eq!(result.cache_entries, 0);
            assert_eq!(result.cache_bytes, 0);
        }
        other => panic!("a Contributor with file.read can inspect bounded cache counts: {other:?}"),
    }

    let denied_clear = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: String::new(),
                action: LspControlActionDto::CacheClear,
            },
        },
    )
    .await;
    assert!(matches!(
        denied_clear,
        CoreResponse::Error { ref code, .. } if code == "authorization_denied"
    ));

    let cleared = call(
        &fixture.daemon,
        MAINTAINER,
        CoreRequest::LspControl {
            request: LspControlRequestDto {
                session_id: fixture.session_id.clone(),
                server_id: String::new(),
                action: LspControlActionDto::CacheClear,
            },
        },
    )
    .await;
    match cleared {
        CoreResponse::LspControlResult { result } => {
            assert_eq!(result.cache_entries, 0);
            assert_eq!(result.cache_bytes, 0);
        }
        other => panic!("maintainer cache clear should remain workspace-scoped: {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_lsp_root_diagnosis_is_session_scoped_and_rejects_escape() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    let response = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspWorkflowExecute {
            request: LspWorkflowRequestDto {
                session_id: fixture.session_id.clone(),
                workflow: LspWorkflowKindDto::RootDiagnosis,
                primary_path: "src/lib.rs".to_owned(),
                related_paths: Vec::new(),
                line: None,
                column: None,
                symbol: None,
                direction: None,
                failure_text: None,
            },
        },
    )
    .await;
    match response {
        CoreResponse::LspWorkflowResult { result } => {
            assert_eq!(result.workflow, LspWorkflowKindDto::RootDiagnosis);
            assert_eq!(result.target, "src/lib.rs");
            assert!(result.rendered.contains("Inside allowed root: yes"));
        }
        other => panic!("expected bounded root diagnosis, got {other:?}"),
    }

    let escaped = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspWorkflowExecute {
            request: LspWorkflowRequestDto {
                session_id: fixture.session_id.clone(),
                workflow: LspWorkflowKindDto::RootDiagnosis,
                primary_path: "../outside.rs".to_owned(),
                related_paths: Vec::new(),
                line: None,
                column: None,
                symbol: None,
                direction: None,
                failure_text: None,
            },
        },
    )
    .await;
    match escaped {
        CoreResponse::Error { code, .. } => assert_eq!(code, "lsp_workflow_path_unavailable"),
        other => panic!("a path outside the session workspace must be refused, got {other:?}"),
    }
    assert!(fixture.service.client_keys().await.is_empty());
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_lsp_root_diagnosis_rejects_symlink_escape() {
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    let outside = TempDir::new().expect("create an outside workspace");
    let outside_file = outside.path().join("outside.rs");
    std::fs::write(&outside_file, "pub fn outside() {}\n").expect("write outside file");
    std::os::unix::fs::symlink(&outside_file, fixture.root.join("src/escape.rs"))
        .expect("create an in-root symlink to outside content");

    let response = call(
        &fixture.daemon,
        MEMBER,
        CoreRequest::LspWorkflowExecute {
            request: LspWorkflowRequestDto {
                session_id: fixture.session_id.clone(),
                workflow: LspWorkflowKindDto::RootDiagnosis,
                primary_path: "src/escape.rs".to_owned(),
                related_paths: Vec::new(),
                line: None,
                column: None,
                symbol: None,
                direction: None,
                failure_text: None,
            },
        },
    )
    .await;

    assert!(matches!(
        response,
        CoreResponse::Error { ref code, .. } if code == "lsp_workflow_path_unavailable"
    ));
    assert!(fixture.service.client_keys().await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn daemon_review_workflow_uses_the_session_lsp_service() {
    let fixture = fixture(RustBehaviour::Workflow, go_only_warming()).await;
    std::fs::write(
        fixture.root.join("src/other.rs"),
        "pub fn beta() -> u32 { 2 }\n",
    )
    .expect("create a second workspace file");
    fixture.open_lib().await;
    let response = call(
        &fixture.daemon,
        MEMBER,
        review_command(&fixture, "src/lib.rs"),
    )
    .await;
    let response = codegg::tui::command_route::unwrap_inner_response(response);
    match response {
        CoreResponse::LspWorkflowResult { result } => {
            assert_eq!(result.workflow, LspWorkflowKindDto::ReviewFile);
            assert_eq!(result.target, "src/lib.rs");
            assert!(
                !result.rendered.trim().is_empty(),
                "review workflow should return bounded human-readable evidence"
            );
            assert!(result.evidence_count <= 30);
        }
        other => panic!("expected bounded review evidence, got {other:?}"),
    }
    let peer_response = call(
        &fixture.daemon,
        PEER,
        review_command(&fixture, "src/other.rs"),
    )
    .await;
    let peer_response = codegg::tui::command_route::unwrap_inner_response(peer_response);
    match peer_response {
        CoreResponse::LspWorkflowResult { result } => {
            assert_eq!(result.workflow, LspWorkflowKindDto::ReviewFile);
            assert_eq!(result.target, "src/other.rs");
        }
        other => panic!("the second authorized frontend must share the daemon workflow: {other:?}"),
    }
    assert_eq!(
        fixture.service.client_keys().await.len(),
        1,
        "two authorized clients reviewing two files must share one warm server"
    );
    let transcript = std::fs::read_to_string(fixture.root.join("rust-analyzer-transcript.jsonl"))
        .expect("fake server records its protocol transcript");
    assert_eq!(
        transcript.matches("textDocument/documentSymbol").count(),
        2,
        "the same server must answer each frontend's file-scoped review"
    );
}

fn review_command(fixture: &Fixture, path: &str) -> CoreRequest {
    codegg::tui::command_route::command_execute(
        codegg::protocol::core::CommandCatalogScopeDto {
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            session_id: Some(fixture.session_id.clone()),
        },
        "lsp_review_file",
        "",
        path,
    )
}

fn read_request(fixture: &Fixture, server_id: Option<&str>) -> LspReadRequestDto {
    LspReadRequestDto {
        operation: LspReadOperation::Hover,
        session_id: fixture.session_id.clone(),
        path: "src/lib.rs".to_string(),
        line: Some(0),
        column: Some(8),
        query: None,
        server_id: server_id.map(str::to_string),
        new_name: None,
    }
}

fn as_read(response: CoreResponse) -> LspReadResultDto {
    match response {
        CoreResponse::LspReadResult { result } => result,
        other => panic!("expected an LSP read result, got {other:?}"),
    }
}

fn candidates(result: &LspReadResultDto) -> Vec<String> {
    match result.payload.as_ref() {
        Some(LspReadPayloadDto::AmbiguousClients { candidates }) => candidates.clone(),
        other => panic!("expected candidates, got {other:?}"),
    }
}

/// Warm one client per language, both rooted at the workspace.
async fn warm_both_servers(fixture: &Fixture) {
    fixture
        .service
        .get_or_create_client(&fixture.root.join("src/lib.rs"))
        .await
        .expect("warm the Rust server");
    fixture
        .service
        .get_or_create_client(&fixture.root.join("src/main.go"))
        .await
        .expect("warm the Go server");
    let keys = fixture.service.client_keys().await;
    assert!(
        keys.len() >= 2,
        "both servers must be warm for this case to mean anything: {keys:?}"
    );
}

/// The Go server's steps for a scenario that never queries it.
fn go_only_warming() -> Vec<Value> {
    vec![init_step(), initialized_step(vec![])]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_warm_sessions_diagnostics_reach_the_stream_without_a_frontend_asking() {
    // M013 W1. Before this milestone nothing called
    // `publish_changed_diagnostics`, so no amount of waiting produced this
    // event: the daemon had a correct publisher and no driver.
    let fixture = fixture(RustBehaviour::Diagnostics, go_only_warming()).await;
    let mut events = fixture.daemon.event_log.subscribe();

    // Warm exactly one server, so the driver has one session with a warm
    // client and nothing else to publish.
    fixture.open_lib().await;

    // Poll the cadence rather than sleeping through the real interval: the
    // point is that *one pass of the real driver* finds the work, not that
    // the interval happens to be short.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = None;
    while Instant::now() < deadline && seen.is_none() {
        fixture.daemon.publish_diagnostics_once().await;
        match tokio::time::timeout(Duration::from_millis(250), events.recv()).await {
            Ok(Ok(envelope)) => {
                if let CoreEvent::LspDiagnosticsUpdated {
                    session_id,
                    project_id,
                    file,
                } = envelope.payload
                {
                    seen = Some((session_id, project_id, file));
                }
            }
            Ok(Err(_)) => break,
            Err(_) => {}
        }
    }

    let (session_id, project_id, file) = seen.expect(
        "a warm session's diagnostics must reach the stream once the daemon \
         drives the publisher; before M013 nothing ever called it",
    );
    assert_eq!(session_id, fixture.session_id);
    assert_eq!(project_id, fixture.project_id);
    assert_eq!(
        file.path, "src/lib.rs",
        "diagnostics are keyed by workspace-relative path, not by file name"
    );
    assert!(
        !file.diagnostics.is_empty(),
        "the published set is the complete current set for the file"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_resync_authority_becomes_ready_once_the_driver_has_run() {
    // The pull and the push must agree. Before M013 the pull was permanently
    // `not_ready` because nothing ever published, so a client that fell back
    // to the authority saw "still warming" forever.
    let fixture = fixture(RustBehaviour::Diagnostics, go_only_warming()).await;
    fixture.open_lib().await;

    let mut events = fixture.daemon.event_log.subscribe();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        fixture.daemon.publish_diagnostics_once().await;
        let _ = tokio::time::timeout(Duration::from_millis(250), events.recv()).await;
        if Instant::now() >= deadline {
            break;
        }
        // Once the store has the file, the authority can answer Ready.
        let response = local_call(
            &fixture.daemon,
            "diag",
            CoreRequest::LspDiagnosticsGet {
                request: LspDiagnosticsGetRequestDto {
                    project_id: fixture.project_id.clone(),
                    session_id: fixture.session_id.clone(),
                },
            },
        )
        .await;
        if let CoreResponse::LspDiagnosticsGetResult { result } = response {
            if result.status == LspDiagnosticsStatus::Ready {
                assert!(
                    !result.files.is_empty(),
                    "a ready authority with no files would be a lie: readiness \
                     means this project has observed data"
                );
                return;
            }
        }
    }
    panic!("the resync authority never became ready once the driver ran");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn several_warm_servers_are_ambiguous_not_not_ready() {
    // M013 W2. This is the defect: the resolver's "several clients" error was
    // mapped to `NotReady`, so two healthy servers and no servers were the
    // same answer with the same absent payload.
    //
    // Workspace symbols, deliberately. A *point* read carries a target whose
    // extension can settle the choice, and this milestone settles it -- see
    // `the_targets_extension_selects_one_of_two_warm_servers`. A
    // project-wide read has no target and no language to infer, so it is the
    // read that genuinely cannot be answered without choosing. It is also the
    // read that used to be routed to `first_client_key()`, which made the
    // answer depend on hash-map iteration order rather than on the request.
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;
    warm_both_servers(&fixture).await;

    let result = as_read(
        call(
            &fixture.daemon,
            MEMBER,
            CoreRequest::LspReadGet {
                request: LspReadRequestDto {
                    operation: LspReadOperation::WorkspaceSymbols,
                    session_id: fixture.session_id.clone(),
                    path: String::new(),
                    line: None,
                    column: None,
                    query: Some("alpha".to_string()),
                    server_id: None,
                    new_name: None,
                },
            },
        )
        .await,
    );

    assert_eq!(
        result.status,
        LspReadStatus::Ambiguous,
        "several warm servers are not a readiness state; retrying cannot make \
         the ambiguity go away"
    );
    let candidates = candidates(&result);
    assert!(
        candidates.contains(&SERVER.to_string()) && candidates.contains(&OTHER_SERVER.to_string()),
        "the ambiguity must name both servers so a client can show the choice: \
         {candidates:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_server_id_resolves_what_the_extension_cannot() {
    // A `.rs` read in a polyglot root is ambiguous, but naming the server is
    // the caller's answer to it. This is what makes `Ambiguous` actionable
    // rather than a dead end.
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;
    warm_both_servers(&fixture).await;

    let result = as_read(
        call(
            &fixture.daemon,
            MEMBER,
            CoreRequest::LspReadGet {
                request: read_request(&fixture, Some(SERVER)),
            },
        )
        .await,
    );

    assert_eq!(result.status, LspReadStatus::Ready);
    match result.payload {
        Some(LspReadPayloadDto::Hover { text, .. }) => {
            assert_eq!(
                text, "from rust",
                "the named server must be the one that answered"
            )
        }
        other => panic!("expected hover text, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_but_cold_server_is_not_ready_and_not_ambiguous() {
    // "No server for what you asked for" and "several servers" are different
    // facts. Reporting the first as the second would tell a user to choose
    // when there is nothing to choose from.
    let fixture = fixture(RustBehaviour::Diagnostics, go_only_warming()).await;
    fixture.open_lib().await;

    let result = as_read(
        call(
            &fixture.daemon,
            MEMBER,
            CoreRequest::LspReadGet {
                request: read_request(&fixture, Some("not-a-server")),
            },
        )
        .await,
    );

    assert_eq!(result.status, LspReadStatus::NotReady);
    assert_eq!(result.payload, None, "a not-ready read carries no answer");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_targets_extension_selects_one_of_two_warm_servers() {
    // The polyglot case the extension *can* settle: `.rs` is claimed by one
    // warm server, so a Rust read is answerable without asking the user.
    let fixture = fixture(RustBehaviour::Hover, go_only_warming()).await;
    fixture.open_lib().await;
    warm_both_servers(&fixture).await;

    let result = as_read(
        call(
            &fixture.daemon,
            MEMBER,
            CoreRequest::LspReadGet {
                request: read_request(&fixture, None),
            },
        )
        .await,
    );

    assert_eq!(
        result.status,
        LspReadStatus::Ready,
        "exactly one warm server claims .rs, so the ambiguity the workspace \
         symbols query would hit does not apply to a point read"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cold_root_still_answers_not_ready_and_starts_no_process() {
    // ADR-0012 §3. A daemon with LSP disabled can never launch a server, so
    // this proves the answer without racing a process.
    let temp = TempDir::new().unwrap();
    let root = temp.path().to_path_buf();
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname='m013'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn alpha() -> u32 { 1 }\n").unwrap();

    let service = LspService::new_arc(LspConfig::Disabled(true));
    let pool = test_pool().await;
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool.clone()), None, None).with_lsp_service(service.clone()),
    );
    let workspace_id = match local_call(
        &daemon,
        "ws",
        CoreRequest::WorkspaceRegister {
            root: root.display().to_string(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match local_call(
        &daemon,
        "proj",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "m013".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, ?, 1, 1, '[]')",
    )
    .bind(&project_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    let session_id = format!("session-{}", uuid::Uuid::new_v4());
    sqlx::query(
        "INSERT INTO session (id, project_id, workspace_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, ?, 's', ?, 't', 'v', 1, 1)",
    )
    .bind(&session_id)
    .bind(&project_id)
    .bind(&workspace_id)
    .bind(root.display().to_string())
    .execute(&pool)
    .await
    .unwrap();
    daemon
        .bind_runtime_for_session(&session_id)
        .await
        .expect("bind runtime for session");

    let response = local_call(
        &daemon,
        "hover",
        CoreRequest::LspReadGet {
            request: LspReadRequestDto {
                operation: LspReadOperation::Hover,
                session_id: session_id.clone(),
                path: "src/lib.rs".to_string(),
                line: Some(0),
                column: Some(8),
                query: None,
                server_id: None,
                new_name: None,
            },
        },
    )
    .await;
    let result = as_read(response);
    assert_eq!(result.status, LspReadStatus::NotReady);
    assert_eq!(result.payload, None);
    assert!(
        service.client_keys().await.is_empty(),
        "a cold read must not leave a client behind"
    );

    // And the cadence is inert for a cold session.
    daemon.publish_diagnostics_once().await;
    assert!(
        service.client_keys().await.is_empty(),
        "driving the publisher must not warm anything either"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn m002_records_cold_warm_and_cpu_loaded_lsp_request_latency() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    fn percentile_ms(samples: &mut [Duration], percentile: usize) -> f64 {
        samples.sort_unstable();
        samples[(samples.len() - 1) * percentile / 100].as_secs_f64() * 1000.0
    }
    async fn sample(fixture: &Fixture, coordinated: bool) -> (Vec<Duration>, LspReadStatus) {
        let mut durations = Vec::with_capacity(M002_LATENCY_SAMPLE_COUNT);
        let mut last_status = LspReadStatus::NotReady;
        let coordinator = codegg_client::LspRequestCoordinator::default();
        let scope = codegg_client::LspRequestScope {
            frontend_id: "latency-fixture".into(),
            attachment_id: fixture.session_id.clone(),
            route_generation: 1,
            connection_generation: 1,
            project_id: fixture.project_id.clone(),
            workspace_id: fixture.workspace_id.clone(),
            session_id: fixture.session_id.clone(),
            document_id: None,
            path: Some("src/lib.rs".into()),
            server_id: Some(SERVER.into()),
        };
        let key = codegg_client::LspRequestKey {
            scope: scope.clone(),
            operation: codegg_client::LspRequestOperation::Read(LspReadOperation::Hover),
            document_generation: 1,
            position: Some((0, 8)),
            argument: None,
        };
        for _ in 0..M002_LATENCY_SAMPLE_COUNT {
            let started = Instant::now();
            let ticket = coordinated.then(|| match coordinator.begin(key.clone()) {
                codegg_client::LspRequestStart::Started(ticket) => ticket,
                other => panic!("latency sample was not admitted: {other:?}"),
            });
            if coordinated {
                tokio::time::sleep(codegg_client::LSP_INTERACTIVE_DEBOUNCE).await;
            }
            let result = as_read(
                call(
                    &fixture.daemon,
                    MEMBER,
                    CoreRequest::LspReadGet {
                        request: read_request(fixture, Some(SERVER)),
                    },
                )
                .await,
            );
            if let Some(ticket) = ticket {
                assert!(coordinator.finish(&ticket, &scope, 1));
            }
            durations.push(started.elapsed());
            last_status = result.status;
        }
        (durations, last_status)
    }

    let fixture = fixture(RustBehaviour::LatencySample, go_only_warming()).await;
    let (mut cold_direct, cold_status) = sample(&fixture, false).await;
    let (mut cold_coordinated, coordinated_cold_status) = sample(&fixture, true).await;
    assert_eq!(cold_status, LspReadStatus::NotReady);
    assert_eq!(coordinated_cold_status, LspReadStatus::NotReady);
    assert!(fixture.service.client_keys().await.is_empty());

    let worker_count = thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(2)
        .saturating_sub(1)
        .clamp(1, 4);
    let start_cpu_load = || {
        let running = Arc::new(AtomicBool::new(true));
        let workers: Vec<_> = (0..worker_count)
            .map(|_| {
                let running = Arc::clone(&running);
                thread::spawn(move || {
                    let mut value = 0_u64;
                    while running.load(Ordering::Relaxed) {
                        value = std::hint::black_box(value.wrapping_add(1));
                    }
                    std::hint::black_box(value);
                })
            })
            .collect();
        (running, workers)
    };
    let stop_cpu_load = |running: Arc<AtomicBool>, workers: Vec<thread::JoinHandle<()>>| {
        running.store(false, Ordering::Relaxed);
        for worker in workers {
            worker.join().expect("CPU-load worker exits cleanly");
        }
    };
    let (cold_load, cold_workers) = start_cpu_load();
    let (mut cold_loaded_direct, cold_loaded_status) = sample(&fixture, false).await;
    let (mut cold_loaded_coordinated, coordinated_cold_loaded_status) =
        sample(&fixture, true).await;
    stop_cpu_load(cold_load, cold_workers);
    assert_eq!(cold_loaded_status, LspReadStatus::NotReady);
    assert_eq!(coordinated_cold_loaded_status, LspReadStatus::NotReady);
    assert!(fixture.service.client_keys().await.is_empty());

    fixture.open_lib().await;
    let (mut warm_direct, warm_status) = sample(&fixture, false).await;
    let (mut warm_coordinated, coordinated_warm_status) = sample(&fixture, true).await;
    assert_eq!(warm_status, LspReadStatus::Ready);
    assert_eq!(coordinated_warm_status, LspReadStatus::Ready);

    let (warm_load, warm_workers) = start_cpu_load();
    let (mut warm_loaded_direct, loaded_status) = sample(&fixture, false).await;
    let (mut warm_loaded_coordinated, coordinated_loaded_status) = sample(&fixture, true).await;
    stop_cpu_load(warm_load, warm_workers);
    assert_eq!(loaded_status, LspReadStatus::Ready);
    assert_eq!(coordinated_loaded_status, LspReadStatus::Ready);

    println!(
        "M002_LSP_REQUEST_MS samples={} cpu_load_workers={worker_count} cold_direct_p50={:.3} cold_direct_p95={:.3} cold_shared_p50={:.3} cold_shared_p95={:.3} cold_loaded_direct_p50={:.3} cold_loaded_direct_p95={:.3} cold_loaded_shared_p50={:.3} cold_loaded_shared_p95={:.3} warm_direct_p50={:.3} warm_direct_p95={:.3} warm_shared_p50={:.3} warm_shared_p95={:.3} warm_loaded_direct_p50={:.3} warm_loaded_direct_p95={:.3} warm_loaded_shared_p50={:.3} warm_loaded_shared_p95={:.3}",
        M002_LATENCY_SAMPLE_COUNT,
        percentile_ms(&mut cold_direct, 50),
        percentile_ms(&mut cold_direct, 95),
        percentile_ms(&mut cold_coordinated, 50),
        percentile_ms(&mut cold_coordinated, 95),
        percentile_ms(&mut cold_loaded_direct, 50),
        percentile_ms(&mut cold_loaded_direct, 95),
        percentile_ms(&mut cold_loaded_coordinated, 50),
        percentile_ms(&mut cold_loaded_coordinated, 95),
        percentile_ms(&mut warm_direct, 50),
        percentile_ms(&mut warm_direct, 95),
        percentile_ms(&mut warm_coordinated, 50),
        percentile_ms(&mut warm_coordinated, 95),
        percentile_ms(&mut warm_loaded_direct, 50),
        percentile_ms(&mut warm_loaded_direct, 95),
        percentile_ms(&mut warm_loaded_coordinated, 50),
        percentile_ms(&mut warm_loaded_coordinated, 95),
    );
}
