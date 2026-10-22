//! Team collaboration M001 — HTTP authorization convergence.
//!
//! Adversarial multi-principal suite proving that every authenticated HTTP
//! compatibility route converges on the canonical authorization service.

use std::sync::Arc;

use axum::extract::{Extension, Query};
use axum::Json;

use codegg::config::schema::Config;
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::server::authz::route_disposition_table;
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::identity::ProjectId;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn test_state(pool: sqlx::SqlitePool, daemon: Option<Arc<CoreDaemon>>) -> ServerState {
    ServerState {
        pool,
        mcp_service: Arc::new(tokio::sync::RwLock::new(codegg::mcp::McpService::new())),
        config: Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
        daemon,
        projection_lifecycle_seam: Default::default(),
        connection_task_probe: None,
        probe_factory: None,
        transport_test_config: None,
    }
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

async fn human_token_with_plaintext(
    team: &TeamStore,
    name: &str,
    client_id: &str,
) -> (AuthenticatedPrincipal, String) {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    let principal = tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap();
    (principal, plaintext)
}

async fn register_workspace(daemon: &CoreDaemon, root: &str) -> String {
    match daemon
        .handle_request(new_request(
            format!("req-ws-{root}"),
            CoreRequest::WorkspaceRegister {
                root: root.to_owned(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        _ => panic!("workspace register failed"),
    }
}

async fn register_project(daemon: &CoreDaemon, workspace_id: String, name: &str) -> String {
    match daemon
        .handle_request(new_request(
            format!("req-reg-{name}"),
            CoreRequest::ProjectRegister {
                request: codegg::protocol::dto::ProjectRegisterRequestDto {
                    workspace_id,
                    display_name: name.to_owned(),
                    description: None,
                    tags: Vec::new(),
                    repository_id: None,
                    source: "test".to_owned(),
                },
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        _ => panic!("project register failed"),
    }
}

fn is_not_found(err: &codegg::error::AxumAppError) -> bool {
    matches!(
        &err.0,
        codegg_core::error::AppError::Storage(codegg_core::error::StorageError::NotFound(_))
    )
}

struct Fixture {
    pool: sqlx::SqlitePool,
    state: ServerState,
    project_a: String,
    project_b: String,
    workspace_a: String,
    workspace_b: String,
    owner_a: AuthenticatedPrincipal,
    viewer_a: AuthenticatedPrincipal,
    contributor_a: AuthenticatedPrincipal,
    outsider: AuthenticatedPrincipal,
    local_owner: AuthenticatedPrincipal,
    _tmp_a: tempfile::TempDir,
    _tmp_b: tempfile::TempDir,
}

async fn fixture() -> Fixture {
    let pool = test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None));
    let team = TeamStore::new(pool.clone());

    let tmp_a = tempfile::tempdir().expect("tempdir");
    let tmp_b = tempfile::tempdir().expect("tempdir");
    let ws_a = register_workspace(&daemon, &tmp_a.path().display().to_string()).await;
    let ws_b = register_workspace(&daemon, &tmp_b.path().display().to_string()).await;
    let project_a = register_project(&daemon, ws_a.clone(), "alpha").await;
    let project_b = register_project(&daemon, ws_b.clone(), "beta").await;

    let owner_a = human_token(&team, "OwnerA", "client-owner-a").await;
    let viewer_a = human_token(&team, "ViewerA", "client-viewer-a").await;
    let contributor_a = human_token(&team, "ContributorA", "client-contrib-a").await;
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    let project_a_id = ProjectId::parse(&project_a).unwrap();
    team.create_membership(&project_a_id, owner_a.principal_id(), ProjectRole::Owner)
        .await
        .unwrap();
    team.create_membership(&project_a_id, viewer_a.principal_id(), ProjectRole::Viewer)
        .await
        .unwrap();
    team.create_membership(
        &project_a_id,
        contributor_a.principal_id(),
        ProjectRole::Contributor,
    )
    .await
    .unwrap();

    let local_owner = AuthenticatedPrincipal::local_owner("client-local");
    let state = test_state(pool.clone(), Some(daemon));
    Fixture {
        pool,
        state,
        project_a,
        project_b,
        workspace_a: ws_a,
        workspace_b: ws_b,
        owner_a,
        viewer_a,
        contributor_a,
        outsider,
        local_owner,
        _tmp_a: tmp_a,
        _tmp_b: tmp_b,
    }
}

fn scope_for(project: &str, workspace: &str) -> codegg::server::scope::ScopeQuery {
    codegg::server::scope::ScopeQuery {
        project_id: Some(project.to_owned()),
        workspace_id: Some(workspace.to_owned()),
        directory: None,
    }
}

async fn create_session_as(
    fx: &Fixture,
    principal: &AuthenticatedPrincipal,
    project: &str,
    workspace: &str,
) -> String {
    let req = codegg::server::routes::session::CreateSessionRequest {
        project_id: Some(project.to_owned()),
        directory: String::new(),
        title: Some("t".to_owned()),
        parent_id: None,
        workspace_id: Some(workspace.to_owned()),
        agent: None,
        model: None,
        tags: None,
    };
    let result = codegg::server::routes::session::create_session(
        Extension(principal.clone()),
        axum::extract::State(fx.state.clone()),
        Json(req),
    )
    .await;
    let (_, Json(session)) = match result {
        Ok(v) => v,
        Err(_) => panic!("authorized session create must succeed"),
    };
    session.id
}

#[tokio::test(flavor = "current_thread")]
async fn every_authenticated_route_has_disposition() {
    let table = route_disposition_table();
    for required in [
        ("GET", "/api/projects"),
        ("GET", "/api/sessions"),
        ("POST", "/api/sessions"),
        ("GET", "/api/sessions/{id}"),
        ("GET", "/api/file/read"),
        ("POST", "/api/file/write"),
        ("GET", "/api/permission/{session_id}"),
        ("POST", "/api/permission/{session_id}/submit"),
        ("GET", "/api/question/{session_id}"),
        ("POST", "/api/question/{session_id}"),
        ("GET", "/api/event"),
        ("GET", "/api/config"),
        ("GET", "/api/providers"),
        ("GET", "/api/tools"),
        ("GET", "/api/mcp"),
        ("POST", "/api/v1/task-triggers/{trigger_id}/fire"),
    ] {
        assert!(
            table
                .iter()
                .any(|e| e.method == required.0 && e.path == required.1),
            "missing disposition"
        );
    }
    assert!(table.len() >= 30, "disposition table breadth");
}

#[tokio::test(flavor = "current_thread")]
async fn project_enumeration_filters_by_membership() {
    let fx = fixture().await;
    let query = codegg::server::routes::project::ProjectListQuery {
        include_archived: false,
        limit: 0,
    };
    let result = codegg::server::routes::project::list_projects(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query.clone()),
    )
    .await;
    let Json(owner_rows) = match result {
        Ok(v) => v,
        Err(_) => panic!("owner lists"),
    };
    assert_eq!(owner_rows.projects.len(), 1);
    assert_eq!(owner_rows.projects[0].id, fx.project_a);

    let result = codegg::server::routes::project::list_projects(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query.clone()),
    )
    .await;
    let Json(outsider_rows) = match result {
        Ok(v) => v,
        Err(_) => panic!("outsider lists"),
    };
    assert!(outsider_rows.projects.is_empty());

    let result = codegg::server::routes::project::list_projects(
        Extension(fx.local_owner.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query),
    )
    .await;
    let Json(local_rows) = match result {
        Ok(v) => v,
        Err(_) => panic!("local lists"),
    };
    assert_eq!(local_rows.projects.len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn cross_project_get_denies_as_not_found() {
    let fx = fixture().await;
    let result = codegg::server::routes::project::get_project_by_id(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(fx.project_b.clone()),
        Query(codegg::server::scope::ScopeQuery::default()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider get must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::project::get_project_by_id(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(fx.project_b.clone()),
        Query(codegg::server::scope::ScopeQuery::default()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("cross-project get must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::project::get_project_by_id(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(fx.project_a.clone()),
        Query(codegg::server::scope::ScopeQuery::default()),
    )
    .await;
    let Json(info) = match result {
        Ok(v) => v,
        Err(_) => panic!("owner reads own project"),
    };
    assert_eq!(info.id, fx.project_a);
}

#[tokio::test(flavor = "current_thread")]
async fn cross_project_session_denies_with_zero_side_effect() {
    let fx = fixture().await;
    let session_a = create_session_as(&fx, &fx.owner_a, &fx.project_a, &fx.workspace_a).await;

    let result = codegg::server::routes::session::get_session(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider session get must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::session::get_session(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let Json(got) = match result {
        Ok(v) => v,
        Err(_) => panic!("viewer reads"),
    };
    assert_eq!(got.id, session_a);

    let store = codegg::session::SessionStore::new(fx.pool.clone());
    let before = store
        .list_by_canonical_project(&fx.project_a, Some(100))
        .await
        .unwrap()
        .len();
    let req = codegg::server::routes::session::CreateSessionRequest {
        project_id: Some(fx.project_a.clone()),
        directory: String::new(),
        title: Some("viewer-attempt".to_owned()),
        parent_id: None,
        workspace_id: Some(fx.workspace_a.clone()),
        agent: None,
        model: None,
        tags: None,
    };
    let result = codegg::server::routes::session::create_session(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        Json(req),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("viewer create must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    let after = store
        .list_by_canonical_project(&fx.project_a, Some(100))
        .await
        .unwrap()
        .len();
    assert_eq!(before, after);

    let result = codegg::server::routes::session::archive_session(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider archive must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    assert!(store.get(&session_a).await.unwrap().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn session_mutations_require_owning_project_grant() {
    let fx = fixture().await;
    let session_a = create_session_as(&fx, &fx.owner_a, &fx.project_a, &fx.workspace_a).await;

    let result = codegg::server::routes::session::fork_session(
        Extension(fx.contributor_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("contributor forks"),
    }
    let result = codegg::server::routes::session::fork_session(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider fork must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::session::share_session(
        Extension(fx.contributor_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("contributor share must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    let result = codegg::server::routes::session::share_session(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(scope_for(&fx.project_a, &fx.workspace_a)),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("owner shares"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn file_scope_denies_cross_project_with_zero_side_effect() {
    let fx = fixture().await;
    let write_req = codegg::server::routes::file::WriteFileRequest {
        path: "seed.txt".to_owned(),
        content: "alpha".to_owned(),
        project_id: Some(fx.project_a.clone()),
        workspace_id: Some(fx.workspace_a.clone()),
        directory: None,
    };
    let result = codegg::server::routes::file::write_file(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        Json(write_req),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("owner writes"),
    }

    let result = codegg::server::routes::file::read_file(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(codegg::server::routes::file::ReadFileQuery {
            path: "seed.txt".to_owned(),
            project_id: Some(fx.project_b.clone()),
            workspace_id: Some(fx.workspace_b.clone()),
            directory: None,
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("cross-project file read must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::file::write_file(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Json(codegg::server::routes::file::WriteFileRequest {
            path: "outsider.txt".to_owned(),
            content: "x".to_owned(),
            project_id: Some(fx.project_a.clone()),
            workspace_id: Some(fx.workspace_a.clone()),
            directory: None,
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider write must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::file::write_file(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        Json(codegg::server::routes::file::WriteFileRequest {
            path: "viewer.txt".to_owned(),
            content: "x".to_owned(),
            project_id: Some(fx.project_a.clone()),
            workspace_id: Some(fx.workspace_a.clone()),
            directory: None,
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("viewer write must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::file::read_file(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(codegg::server::routes::file::ReadFileQuery {
            path: "seed.txt".to_owned(),
            project_id: None,
            workspace_id: None,
            directory: None,
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("scope-free team file read must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
}

#[tokio::test(flavor = "current_thread")]
async fn pending_control_ids_do_not_leak_across_projects() {
    let fx = fixture().await;
    let session_a = create_session_as(&fx, &fx.owner_a, &fx.project_a, &fx.workspace_a).await;

    // M004 (ADR-0007, accepted): control responses narrow to the
    // active-turn controller lease — no lease means no one may respond
    // until an explicit recovery takeover. The pending items below
    // therefore carry their owning turn and the owner holds the matching
    // durable lease; outsider/viewer probes still deny as 404 at the
    // capability gate before the lease is ever consulted.
    let turn_id = "turn-m001-pending".to_owned();
    codegg_core::session_control::SessionControllerStore::new(fx.pool.clone())
        .acquire(
            &session_a,
            &turn_id,
            fx.owner_a.principal_id(),
            Some("client-owner-a"),
            1,
        )
        .await
        .expect("owner controller lease");
    let (tx, _rx) = tokio::sync::oneshot::channel();
    codegg_core::bus::PermissionRegistry::register_with_session(
        session_a.clone(),
        Some(turn_id.clone()),
        "perm-1".to_owned(),
        tx,
    );
    let (qtx, _qrx) = tokio::sync::oneshot::channel();
    codegg_core::bus::QuestionRegistry::register_with_session(
        session_a.clone(),
        Some(turn_id.clone()),
        "q-1".to_owned(),
        qtx,
    );

    let result = codegg::server::routes::permission::get_pending_permissions(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider perm list must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    let result = codegg::server::routes::question::get_pending_questions(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider question list must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::permission::get_pending_permissions(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(session_a.clone()),
    )
    .await;
    let Json(perms) = match result {
        Ok(v) => v,
        Err(_) => panic!("owner perm list"),
    };
    assert_eq!(perms["permissions"].as_array().unwrap().len(), 1);

    let result = codegg::server::routes::permission::submit_permission(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path("perm-1".to_owned()),
        Json(
            codegg::server::routes::permission::SubmitPermissionRequest {
                session_id: session_a.clone(),
                tool: "bash".to_owned(),
                decision: "allow".to_owned(),
                persist: false,
                perm_id: None,
            },
        ),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("viewer perm respond must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    assert!(codegg_core::bus::PermissionRegistry::is_registered_scoped(
        &session_a, "perm-1"
    ));

    let result = codegg::server::routes::permission::submit_permission(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path("perm-1".to_owned()),
        Json(
            codegg::server::routes::permission::SubmitPermissionRequest {
                session_id: session_a.clone(),
                tool: "bash".to_owned(),
                decision: "allow".to_owned(),
                persist: false,
                perm_id: None,
            },
        ),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("owner perm respond"),
    }
    assert!(!codegg_core::bus::PermissionRegistry::is_registered_scoped(
        &session_a, "perm-1"
    ));

    let result = codegg::server::routes::question::submit_question(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(session_a.clone()),
        Json(codegg::server::routes::question::SubmitQuestionRequest {
            session_id: session_a.clone(),
            answers: serde_json::json!(["a"]),
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider question respond must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    let result = codegg::server::routes::question::submit_question(
        Extension(fx.owner_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(session_a.clone()),
        Json(codegg::server::routes::question::SubmitQuestionRequest {
            session_id: session_a.clone(),
            answers: serde_json::json!(["a"]),
        }),
    )
    .await;
    let Json(resp) = match result {
        Ok(v) => v,
        Err(_) => panic!("owner question respond"),
    };
    assert_eq!(resp.status, "answered");
}

#[tokio::test(flavor = "current_thread")]
async fn sse_and_global_surfaces_are_local_owner_only() {
    let fx = fixture().await;
    let result = codegg::server::routes::event::sse_handler(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("team SSE must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
    let result = codegg::server::routes::event::sse_handler(
        Extension(fx.local_owner.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("local SSE"),
    }

    let result = codegg::server::routes::config::get_config(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("team config must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::provider::list_providers(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("team providers must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::tool::list_tools(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("team tools must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::mcp::list_mcp_servers(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("team mcp must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));

    let result = codegg::server::routes::provider::list_providers(
        Extension(fx.local_owner.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("local providers"),
    }
    let result = codegg::server::routes::tool::list_tools(
        Extension(fx.local_owner.clone()),
        axum::extract::State(fx.state.clone()),
    )
    .await;
    match result {
        Ok(_) => {}
        Err(_) => panic!("local tools"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn revocation_applies_on_next_request_without_reconnect() {
    let fx = fixture().await;
    let team = TeamStore::new(fx.pool.clone());
    let query = codegg::server::routes::project::ProjectListQuery {
        include_archived: false,
        limit: 0,
    };
    let result = codegg::server::routes::project::list_projects(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query.clone()),
    )
    .await;
    let Json(before) = match result {
        Ok(v) => v,
        Err(_) => panic!("viewer lists before revocation"),
    };
    assert_eq!(before.projects.len(), 1);

    let project_a_id = ProjectId::parse(&fx.project_a).unwrap();
    let membership = team
        .get_membership(&project_a_id, fx.viewer_a.principal_id())
        .await
        .unwrap()
        .expect("membership");
    team.revoke_membership(
        &project_a_id,
        fx.viewer_a.principal_id(),
        membership.revision,
    )
    .await
    .unwrap();
    let result = codegg::server::routes::project::list_projects(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query),
    )
    .await;
    let Json(after) = match result {
        Ok(v) => v,
        Err(_) => panic!("enumeration still answers"),
    };
    assert!(after.projects.is_empty());
    let result = codegg::server::routes::project::get_project_by_id(
        Extension(fx.viewer_a.clone()),
        axum::extract::State(fx.state.clone()),
        axum::extract::Path(fx.project_a.clone()),
        Query(codegg::server::scope::ScopeQuery::default()),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("revoked get must deny"),
        Err(e) => e,
    };
    assert!(is_not_found(&err));
}

#[tokio::test(flavor = "current_thread")]
async fn task_trigger_bearer_never_binds_a_principal() {
    let pool = test_pool().await;
    let config = Config::default();
    let trigger_shaped = "cggtr_trigger-id-1234567890.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert!(codegg_core::work_order::is_task_trigger_presentation(
        trigger_shaped
    ));
    let result = codegg::server::middleware::auth::resolve_bearer_principal(
        trigger_shaped,
        &config,
        &pool,
        "client-trigger",
    )
    .await;
    match result {
        Ok(_) => panic!("trigger bearer must not bind a principal"),
        Err(status) => assert!(status.as_u16() == 401 || status.as_u16() == 503),
    }

    let team = TeamStore::new(pool.clone());
    let (_, personal_plaintext) = human_token_with_plaintext(&team, "Human", "client-human").await;
    assert!(!codegg_core::work_order::is_task_trigger_presentation(
        &personal_plaintext
    ));
}
