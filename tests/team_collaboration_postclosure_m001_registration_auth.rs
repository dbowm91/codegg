//! Team Collaboration Post-Closure M001 — registration authority boundary.
//!
//! Adversarial regression proving raw daemon-local workspace/project
//! bootstrap is LocalOwner/proven-local only. Ordinary team principals
//! (Owner/Contributor/Viewer on an unrelated project, plus an outsider
//! with no grants) fail closed with zero filesystem/catalog/membership
//! side effects through both Core and HTTP. LocalOwner bootstrap and
//! project-scoped `POST /api/workspace` remain functional.

use std::sync::Arc;

use axum::extract::{Extension, Query};
use axum::Json;

use codegg::config::schema::Config;
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::authorization::{operation_descriptor, ScopeKind};
use codegg_core::identity::ProjectId;
use codegg_core::project_catalog::ProjectCatalog;
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

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

async fn call_core(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{client}-{}", uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

fn core_error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

fn http_is_not_found(err: &codegg::error::AxumAppError) -> bool {
    matches!(
        &err.0,
        codegg_core::error::AppError::Storage(codegg_core::error::StorageError::NotFound(_))
    )
}

async fn local_workspace_register(daemon: &CoreDaemon, root: &str) -> String {
    match daemon
        .handle_request(new_request(
            format!("req-local-ws-{root}"),
            CoreRequest::WorkspaceRegister {
                root: root.to_owned(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("local workspace register failed: {other:?}"),
    }
}

async fn local_project_register(daemon: &CoreDaemon, workspace_id: &str, name: &str) -> String {
    match daemon
        .handle_request(new_request(
            format!("req-local-reg-{name}"),
            CoreRequest::ProjectRegister {
                request: codegg::protocol::dto::ProjectRegisterRequestDto {
                    workspace_id: workspace_id.to_owned(),
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
        other => panic!("local project register failed: {other:?}"),
    }
}

struct World {
    pool: sqlx::SqlitePool,
    daemon: Arc<CoreDaemon>,
    state: ServerState,
    workspace_id: String,
    project_id: String,
    owner: AuthenticatedPrincipal,
    contributor: AuthenticatedPrincipal,
    viewer: AuthenticatedPrincipal,
    outsider: AuthenticatedPrincipal,
    local_owner: AuthenticatedPrincipal,
    _tmp: tempfile::TempDir,
}

async fn world() -> World {
    let pool = test_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None));
    let team = TeamStore::new(pool.clone());
    let tmp = tempfile::tempdir().expect("tempdir");
    let ws = local_workspace_register(&daemon, &tmp.path().display().to_string()).await;
    let project = local_project_register(&daemon, &ws, "seed").await;

    let owner = human_token(&team, "SeedOwner", "client-owner").await;
    let contributor = human_token(&team, "SeedContributor", "client-contrib").await;
    let viewer = human_token(&team, "SeedViewer", "client-viewer").await;
    let outsider = human_token(&team, "SeedOutsider", "client-outsider").await;
    let project_id = ProjectId::parse(&project).unwrap();
    team.create_membership(&project_id, owner.principal_id(), ProjectRole::Owner)
        .await
        .unwrap();
    team.create_membership(
        &project_id,
        contributor.principal_id(),
        ProjectRole::Contributor,
    )
    .await
    .unwrap();
    team.create_membership(&project_id, viewer.principal_id(), ProjectRole::Viewer)
        .await
        .unwrap();

    for (client, principal) in [
        ("client-owner", owner.clone()),
        ("client-contrib", contributor.clone()),
        ("client-viewer", viewer.clone()),
        ("client-outsider", outsider.clone()),
    ] {
        register_client(&daemon, client, principal);
    }

    let local_owner = AuthenticatedPrincipal::local_owner("client-local");
    let state = test_state(pool.clone(), Some(daemon.clone()));
    World {
        pool,
        daemon,
        state,
        workspace_id: ws,
        project_id: project,
        owner,
        contributor,
        viewer,
        outsider,
        local_owner,
        _tmp: tmp,
    }
}

fn team_clients() -> [(&'static str, &'static str); 4] {
    [
        ("client-owner", "Owner"),
        ("client-contrib", "Contributor"),
        ("client-viewer", "Viewer"),
        ("client-outsider", "outsider"),
    ]
}

async fn workspace_count(daemon: &CoreDaemon) -> usize {
    daemon.workspaces.list(false).await.unwrap().len()
}

async fn project_count(pool: &sqlx::SqlitePool) -> usize {
    ProjectCatalog::new(pool.clone())
        .list_projects(false)
        .await
        .unwrap()
        .len()
}

async fn membership_count(pool: &sqlx::SqlitePool, project: &ProjectId) -> usize {
    TeamStore::new(pool.clone())
        .list_memberships_for_project(project)
        .await
        .unwrap()
        .len()
}

#[test]
fn core_descriptors_are_local_owner_only() {
    let ws_reg = operation_descriptor(&CoreRequest::WorkspaceRegister {
        root: "/tmp/x".to_owned(),
    });
    assert_eq!(ws_reg.operation, "workspace_register");
    assert_eq!(ws_reg.scope_kind, ScopeKind::Opaque);
    assert_eq!(ws_reg.capability_name(), "project.configure");

    let ws_list = operation_descriptor(&CoreRequest::WorkspaceList {
        include_archived: false,
    });
    assert_eq!(ws_list.operation, "workspace_list");
    assert_eq!(ws_list.scope_kind, ScopeKind::Opaque);
    assert_eq!(ws_list.capability_name(), "project.read");

    let proj_reg = operation_descriptor(&CoreRequest::ProjectRegister {
        request: codegg::protocol::dto::ProjectRegisterRequestDto {
            workspace_id: "ws".to_owned(),
            display_name: "n".to_owned(),
            description: None,
            tags: Vec::new(),
            repository_id: None,
            source: "t".to_owned(),
        },
    });
    assert_eq!(proj_reg.operation, "project_register");
    assert_eq!(proj_reg.scope_kind, ScopeKind::Opaque);
    assert_eq!(proj_reg.capability_name(), "project.configure");
}

#[tokio::test(flavor = "current_thread")]
async fn team_workspace_register_denied_with_zero_side_effect() {
    let fx = world().await;
    for (client, label) in team_clients() {
        let before = workspace_count(&fx.daemon).await;
        let attempt_dir = fx._tmp.path().join(format!("denied-ws-{label}"));
        assert!(!attempt_dir.exists(), "precondition for {label}");
        let response = call_core(
            &fx.daemon,
            client,
            CoreRequest::WorkspaceRegister {
                root: attempt_dir.display().to_string(),
            },
        )
        .await;
        assert_eq!(
            core_error_code(&response),
            "authorization_scope_required",
            "{label} WorkspaceRegister must fail closed"
        );
        assert!(
            !attempt_dir.exists(),
            "{label} denied register must not create a directory"
        );
        assert_eq!(
            workspace_count(&fx.daemon).await,
            before,
            "{label} denied register must not add a workspace row"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn team_workspace_list_denied_without_root_disclosure() {
    let fx = world().await;
    for (client, label) in team_clients() {
        let response = call_core(
            &fx.daemon,
            client,
            CoreRequest::WorkspaceList {
                include_archived: false,
            },
        )
        .await;
        assert_eq!(
            core_error_code(&response),
            "authorization_scope_required",
            "{label} WorkspaceList must fail closed"
        );
        let rendered = format!("{response:?}");
        assert!(
            !rendered.contains(&fx._tmp.path().display().to_string()),
            "{label} denial must not leak the canonical root"
        );
    }
    // LocalOwner still enumerates through the canonical gate.
    let local = call_core(
        &fx.daemon,
        "client-unregistered-local",
        CoreRequest::WorkspaceList {
            include_archived: false,
        },
    )
    .await;
    match local {
        CoreResponse::WorkspaceList { workspaces } => {
            assert!(!workspaces.is_empty(), "LocalOwner must list workspaces");
        }
        other => panic!("LocalOwner WorkspaceList must succeed, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn team_project_register_denied_with_zero_side_effect() {
    let fx = world().await;
    let project_id = ProjectId::parse(&fx.project_id).unwrap();
    for (client, label) in team_clients() {
        for workspace_probe in [
            fx.workspace_id.clone(),
            "ws-guessed-00000000000000000000000000".to_owned(),
        ] {
            let before_projects = project_count(&fx.pool).await;
            let before_memberships = membership_count(&fx.pool, &project_id).await;
            let response = call_core(
                &fx.daemon,
                client,
                CoreRequest::ProjectRegister {
                    request: codegg::protocol::dto::ProjectRegisterRequestDto {
                        workspace_id: workspace_probe.clone(),
                        display_name: format!("evil-{label}"),
                        description: None,
                        tags: Vec::new(),
                        repository_id: None,
                        source: "evil".to_owned(),
                    },
                },
            )
            .await;
            let code = core_error_code(&response).to_owned();
            assert!(
                code == "authorization_scope_required" || code == "invalid_workspace_id",
                "{label} ProjectRegister against {workspace_probe} must fail closed, got {code}"
            );
            // Guessed IDs parse as invalid; known IDs must deny at the
            // gate. Either way nothing is created.
            assert_eq!(
                project_count(&fx.pool).await,
                before_projects,
                "{label} denied ProjectRegister must not add a project"
            );
            assert_eq!(
                membership_count(&fx.pool, &project_id).await,
                before_memberships,
                "{label} denied ProjectRegister must not add membership"
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn local_owner_core_bootstrap_succeeds() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool), None, None);
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("local-bootstrap");
    std::fs::create_dir_all(&root).unwrap();
    let response = daemon
        .handle_request(new_request(
            "req-local-bootstrap".to_owned(),
            CoreRequest::WorkspaceRegister {
                root: root.display().to_string(),
            },
        ))
        .await
        .unwrap();
    let workspace_id = match response {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("LocalOwner WorkspaceRegister must succeed, got {other:?}"),
    };
    let response = daemon
        .handle_request(new_request(
            "req-local-project".to_owned(),
            CoreRequest::ProjectRegister {
                request: codegg::protocol::dto::ProjectRegisterRequestDto {
                    workspace_id: workspace_id.clone(),
                    display_name: "local".to_owned(),
                    description: None,
                    tags: Vec::new(),
                    repository_id: None,
                    source: "test".to_owned(),
                },
            },
        ))
        .await
        .unwrap();
    match response {
        CoreResponse::ProjectRegistered { project } => {
            assert!(!project.project_id.is_empty());
        }
        other => panic!("LocalOwner ProjectRegister must succeed, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn http_post_project_denied_before_side_effects() {
    let fx = world().await;
    let project_id = ProjectId::parse(&fx.project_id).unwrap();
    let team_principals = [
        fx.owner.clone(),
        fx.contributor.clone(),
        fx.viewer.clone(),
        fx.outsider.clone(),
    ];
    for (index, principal) in team_principals.into_iter().enumerate() {
        for probe in ["missing-path", "existing-path"] {
            let attempt = fx._tmp.path().join(format!("http-denied-{index}-{probe}"));
            if probe == "existing-path" {
                std::fs::create_dir_all(&attempt).unwrap();
            } else {
                assert!(!attempt.exists());
            }
            let before_ws = workspace_count(&fx.daemon).await;
            let before_projects = project_count(&fx.pool).await;
            let before_memberships = membership_count(&fx.pool, &project_id).await;
            let result = codegg::server::routes::project::create_project(
                Extension(principal.clone()),
                axum::extract::State(fx.state.clone()),
                Json(codegg::server::routes::project::CreateProjectRequest {
                    name: format!("evil-{index}-{probe}"),
                    path: attempt.display().to_string(),
                }),
            )
            .await;
            let err = match result {
                Ok(_) => panic!("team POST /api/project must deny for {probe}"),
                Err(e) => e,
            };
            assert!(
                http_is_not_found(&err),
                "team denial must be bounded not-found, got {err:?}"
            );
            if probe == "missing-path" {
                assert!(
                    !attempt.exists(),
                    "denied HTTP register must not mkdir {attempt:?}"
                );
            }
            assert_eq!(
                workspace_count(&fx.daemon).await,
                before_ws,
                "denied HTTP register must not add a workspace"
            );
            assert_eq!(
                project_count(&fx.pool).await,
                before_projects,
                "denied HTTP register must not add a project"
            );
            assert_eq!(
                membership_count(&fx.pool, &project_id).await,
                before_memberships,
                "denied HTTP register must not add membership"
            );
            // Same bounded shape whether or not the path exists.
            let rendered = format!("{err:?}");
            assert!(rendered.contains("not_found"), "privacy-safe denial");
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn http_post_project_local_owner_succeeds() {
    let fx = world().await;
    let attempt = fx._tmp.path().join("http-local-ok");
    assert!(!attempt.exists());
    let (status, Json(info)) = codegg::server::routes::project::create_project(
        Extension(fx.local_owner.clone()),
        axum::extract::State(fx.state.clone()),
        Json(codegg::server::routes::project::CreateProjectRequest {
            name: "local-http".to_owned(),
            path: attempt.display().to_string(),
        }),
    )
    .await
    .expect("LocalOwner POST /api/project must succeed");
    assert_eq!(status, axum::http::StatusCode::CREATED);
    assert!(!info.id.is_empty());
    assert!(
        attempt.exists(),
        "LocalOwner bootstrap creates the directory"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn http_post_workspace_scoped_mutation_unchanged() {
    let fx = world().await;
    // Owner holds `project.configure` on the seed project, so the
    // project-scoped workspace mutation stays available.
    let result = codegg::server::routes::workspace::create_workspace(
        Extension(fx.owner.clone()),
        axum::extract::State(fx.state.clone()),
        Json(codegg::server::routes::workspace::CreateWorkspaceRequest {
            name: "scoped".to_owned(),
            path: "scoped-child".to_owned(),
            project_id: Some(fx.project_id.clone()),
            workspace_id: Some(fx.workspace_id.clone()),
        }),
    )
    .await;
    match result {
        Ok((status, _)) => assert_eq!(status, axum::http::StatusCode::CREATED),
        Err(e) => panic!("scoped workspace mutation must stay available, got {e:?}"),
    }
    // An outsider without the grant still fails closed.
    let result = codegg::server::routes::workspace::create_workspace(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Json(codegg::server::routes::workspace::CreateWorkspaceRequest {
            name: "scoped".to_owned(),
            path: "scoped-child".to_owned(),
            project_id: Some(fx.project_id.clone()),
            workspace_id: Some(fx.workspace_id.clone()),
        }),
    )
    .await;
    let err = match result {
        Ok(_) => panic!("outsider scoped workspace mutation must deny"),
        Err(e) => e,
    };
    assert!(http_is_not_found(&err));
}

#[tokio::test(flavor = "current_thread")]
async fn route_disposition_leaves_no_shared_authz_without_capability() {
    let table = codegg::server::authz::route_disposition_table();
    for entry in &table {
        if entry.disposition == codegg::server::authz::RouteDisposition::SharedAuthz {
            assert_ne!(
                entry.capability, "none",
                "shared_authz route must name a capability: {} {}",
                entry.method, entry.path
            );
        }
    }
    let post_project = table
        .iter()
        .find(|e| e.method == "POST" && e.path == "/api/project")
        .expect("POST /api/project disposition exists");
    assert_eq!(
        post_project.disposition,
        codegg::server::authz::RouteDisposition::LocalOwnerOnly
    );
}

#[tokio::test(flavor = "current_thread")]
async fn team_project_picker_lists_only_granted_projects() {
    let fx = world().await;
    let query = codegg::server::routes::project::ProjectListQuery {
        include_archived: false,
        limit: 0,
    };
    let Json(owner_rows) = codegg::server::routes::project::list_projects(
        Extension(fx.owner.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query.clone()),
    )
    .await
    .expect("owner lists");
    assert_eq!(owner_rows.projects.len(), 1);
    let Json(outsider_rows) = codegg::server::routes::project::list_projects(
        Extension(fx.outsider.clone()),
        axum::extract::State(fx.state.clone()),
        Query(query),
    )
    .await
    .expect("outsider lists");
    assert!(outsider_rows.projects.is_empty());
}
