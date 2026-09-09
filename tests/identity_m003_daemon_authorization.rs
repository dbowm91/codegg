//! Identity M003 — daemon authorization and originating-principal attribution.
//!
//! Boundary proof for the plan: the daemon gate denies unauthorized team
//! requests with zero side effect, enumeration is privacy-filtered,
//! single-project denials are indistinguishable from absence, request DTOs
//! carry no authority, and durable sessions/turns/jobs capture the
//! transport-bound origin.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_core::authorization::{
    authorize_child_delegation, authorize_provider_use, child_escalates, denial_as_not_found,
    is_local_owner_broad, operation_capability_matrix, operation_descriptor, AuthorizationRequest,
    AuthorizationService, Capability, CapabilitySet, OperationDescriptor, OriginAttributionStore,
    ProjectRole, ScopeKind,
};
use codegg_core::identity::ProjectId;
use codegg_core::provider_connections::ProviderScope;
use codegg_core::team::{PrincipalKind, TeamStore};
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

async fn test_daemon(pool: sqlx::SqlitePool) -> CoreDaemon {
    CoreDaemon::new(Some(pool), None, None)
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

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

async fn register_workspace(daemon: &CoreDaemon, root: &str) -> String {
    match Box::pin(daemon.handle_request(new_request(
        format!("req-ws-{root}"),
        CoreRequest::WorkspaceRegister {
            root: root.to_owned(),
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    }
}

async fn register_project(daemon: &CoreDaemon, workspace_id: String, name: &str) -> String {
    match Box::pin(daemon.handle_request(new_request(
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
    )))
    .await
    .unwrap()
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    }
}

async fn list_projects_as(
    daemon: &CoreDaemon,
    client: &str,
) -> Vec<codegg::protocol::dto::ProjectSummaryDto> {
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-list-{client}"),
            CoreRequest::ProjectList {
                include_archived: false,
                limit: 100,
            },
        ),
        client,
    ))
    .await
    .unwrap()
    {
        CoreResponse::ProjectList { projects, .. } => projects,
        other => panic!("project list failed: {other:?}"),
    }
}

async fn get_project_as(daemon: &CoreDaemon, client: &str, project_id: &str) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-get-{client}"),
            CoreRequest::ProjectGet {
                project_id: project_id.to_owned(),
            },
        ),
        client,
    ))
    .await
    .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn matrix_covers_native_surface_without_duplicates() {
    let matrix = operation_capability_matrix();
    assert!(matrix.len() >= 130, "matrix breadth, got {}", matrix.len());
    let mut names: Vec<&str> = matrix.iter().map(|(op, _, _)| op.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), matrix.len());
    // Spot-check the plan's load-bearing rows.
    let find = |operation: &str| {
        matrix
            .iter()
            .find(|(op, _, _)| op == operation)
            .unwrap_or_else(|| panic!("missing {operation}"))
            .clone()
    };
    assert_eq!(find("turn_submit").2, "agent.invoke");
    assert_eq!(
        find("project_list"),
        (
            "project_list".to_owned(),
            "enumeration".to_owned(),
            "project.read".to_owned()
        )
    );
    assert_eq!(find("project_get").2, "project.read");
    assert_eq!(find("session_create").2, "session.create");
    assert_eq!(find("job_submit").2, "job.submit");
    assert_eq!(find("lsp_preview_apply").2, "file.modify");
}

#[tokio::test(flavor = "current_thread")]
async fn request_dtos_carry_no_authority() {
    // A spoofed payload principal must have nowhere to live: the DTOs are
    // locators only. Assert at the wire shape, not just by convention.
    let requests = vec![
        CoreRequest::SessionCreate {
            directory: "/tmp".to_owned(),
            title: None,
            project_id: Some("project-victim".to_owned()),
            workspace_id: None,
        },
        CoreRequest::TurnSubmit {
            session_id: "session-1".to_owned(),
            text: String::new(),
            plan_mode: false,
            model: String::new(),
            agents: Vec::new(),
            current_agent_idx: 0,
            messages: Vec::new(),
        },
        CoreRequest::ProjectGet {
            project_id: "project-victim".to_owned(),
        },
    ];
    for request in &requests {
        let value = serde_json::to_value(request).expect("serialize request");
        let object = value.as_object().expect("request object");
        for forbidden in [
            "principal",
            "principal_id",
            "role",
            "capability",
            "authority",
        ] {
            assert!(
                !object.contains_key(forbidden),
                "request DTO must not carry {forbidden}: {request:?}"
            );
        }
    }
    // And the service constructor only accepts bound transport principals:
    // an attacker bound to no membership is denied for the victim project.
    let pool = test_pool().await;
    let team = TeamStore::new(pool);
    let service = AuthorizationService::new(team.clone());
    let victim = ProjectId::new();
    let attacker = human_token(&team, "Mallory", "client-mallory").await;
    let descriptor = operation_descriptor(&requests[1]);
    let authz = AuthorizationRequest::new(attacker, descriptor, Some(victim), "corr-spoof");
    assert!(service.authorize(&authz).await.is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn owner_observer_control_matrix_at_session_scope() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool);
    let service = AuthorizationService::new(team.clone());
    let project = ProjectId::new();

    let observer = human_token(&team, "Viewer", "client-viewer").await;
    // Token creation grants no membership; grant Viewer explicitly.
    let viewer_principal = team
        .get_principal(observer.principal_id())
        .await
        .unwrap()
        .unwrap();
    let _ = team
        .create_membership(&project, &viewer_principal.id, ProjectRole::Viewer)
        .await
        .unwrap();

    let controller = human_token(&team, "Contributor", "client-controller").await;
    let controller_principal = team
        .get_principal(controller.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &controller_principal.id, ProjectRole::Contributor)
        .await
        .unwrap();

    let decide = |principal: AuthenticatedPrincipal, capability: Capability| {
        let service = service.clone();
        let project = project.clone();
        async move {
            let descriptor =
                OperationDescriptor::new("probe", ScopeKind::ViaSession, Some(capability));
            let request =
                AuthorizationRequest::new(principal, descriptor, Some(project), "corr-matrix");
            service.authorize(&request).await.is_ok()
        }
    };
    // Observer may read but never invoke.
    assert!(decide(observer.clone(), Capability::SessionRead).await);
    assert!(!decide(observer.clone(), Capability::AgentInvoke).await);
    assert!(!decide(observer, Capability::SessionCreate).await);
    // Controller may invoke and create.
    assert!(decide(controller.clone(), Capability::SessionRead).await);
    assert!(decide(controller.clone(), Capability::AgentInvoke).await);
    assert!(decide(controller, Capability::SessionCreate).await);
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_gate_denies_remote_turn_with_zero_side_effect() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();

    // Session row owned by the project (inserted directly; the gate only
    // needs the session->project linkage). The legacy project row satisfies
    // the session table's foreign key.
    sqlx::query(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, '/tmp', 1, 1, '[]')",
    )
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind("session-gated")
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();

    let viewer = human_token(&team, "Viewer", "client-viewer").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-viewer", viewer);

    let denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-denied".to_owned(),
            CoreRequest::TurnSubmit {
                session_id: "session-gated".to_owned(),
                text: "hello".to_owned(),
                plan_mode: false,
                model: "test/model".to_owned(),
                agents: Vec::new(),
                current_agent_idx: 0,
                messages: Vec::new(),
            },
        ),
        "client-viewer",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "authorization_denied");
    // Zero side effect: no turn attribution was captured for the denial.
    let store = OriginAttributionStore::new(pool.clone());
    assert!(store.get("turn", "denied-turn").await.unwrap().is_none());

    // A Contributor on the same session passes the gate (downstream may
    // still fail for unrelated reasons, but never with an auth code).
    let contributor = human_token(&team, "Contributor", "client-controller").await;
    let contributor_id = contributor.principal_id().clone();
    team.create_membership(&project, &contributor_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "client-controller", contributor);
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-allowed".to_owned(),
            CoreRequest::TurnSubmit {
                session_id: "session-gated".to_owned(),
                text: "hello".to_owned(),
                plan_mode: false,
                model: "missing-provider/model".to_owned(),
                agents: Vec::new(),
                current_agent_idx: 0,
                messages: Vec::new(),
            },
        ),
        "client-controller",
    ))
    .await
    .unwrap();
    if let CoreResponse::Error { code, .. } = &response {
        assert!(
            !code.starts_with("authorization_"),
            "gate must pass a granted principal, got {code}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_project_privacy_list_filters_and_get_hides_existence() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());

    // Two catalog projects created through the local (broad-policy) path.
    let root_a = tempfile::tempdir().expect("tempdir");
    let root_b = tempfile::tempdir().expect("tempdir");
    let ws_a = register_workspace(&daemon, &root_a.path().display().to_string()).await;
    let ws_b = register_workspace(&daemon, &root_b.path().display().to_string()).await;
    let project_a = register_project(&daemon, ws_a, "alpha").await;
    let project_b = register_project(&daemon, ws_b, "beta").await;
    assert_ne!(project_a, project_b);

    // Alice observes alpha only; Bob observes nothing.
    let alice = human_token(&team, "Alice", "client-alice").await;
    let bob = human_token(&team, "Bob", "client-bob").await;
    let project_a_id = ProjectId::parse(&project_a).unwrap();
    team.create_membership(&project_a_id, alice.principal_id(), ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-alice", alice);
    register_client(&daemon, "client-bob", bob);

    let alice_projects = list_projects_as(&daemon, "client-alice").await;
    assert_eq!(alice_projects.len(), 1);
    assert_eq!(alice_projects[0].project_id, project_a);
    let bob_projects = list_projects_as(&daemon, "client-bob").await;
    assert!(bob_projects.is_empty(), "outsider must observe nothing");

    // Single-project denial is indistinguishable from absence.
    let denied = get_project_as(&daemon, "client-bob", &project_b).await;
    assert_eq!(error_code(&denied), "project_not_found");
    // Even the member's read of a genuinely absent project looks the same.
    let absent = get_project_as(&daemon, "client-alice", "absent-project-1").await;
    assert_eq!(error_code(&absent), "project_not_found");
    let (code, _) = denial_as_not_found();
    assert_eq!(code, "project_not_found");
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_session_lifecycle_captures_origin_attribution() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let root = tempfile::tempdir().expect("tempdir");
    let workspace_id = match Box::pin(daemon.handle_request(new_request(
        "req-ws".to_owned(),
        CoreRequest::WorkspaceRegister {
            root: root.path().display().to_string(),
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match Box::pin(daemon.handle_request(new_request(
        "req-reg".to_owned(),
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "attributed".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    let session_id = match Box::pin(daemon.handle_request(new_request(
        "req-create".to_owned(),
        CoreRequest::SessionCreate {
            directory: root.path().display().to_string(),
            title: Some("attributed session".to_owned()),
            project_id: Some(project_id),
            workspace_id: Some(workspace_id),
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("session create failed: {other:?}"),
    };
    let store = OriginAttributionStore::new(pool);
    let attribution = store
        .get("session", &session_id)
        .await
        .unwrap()
        .expect("session creation must capture origin attribution");
    assert!(!attribution.is_legacy());
    assert_eq!(attribution.origin_principal.as_str(), "local-owner");
    assert!(!attribution.decision_id.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn membership_revocation_takes_effect_at_boundary() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = human_token(&team, "Owner", "client-owner").await;
    let owner_id = owner.principal_id().clone();
    let membership = team
        .create_membership(&project, &owner_id, ProjectRole::Owner)
        .await
        .unwrap();
    register_client(&daemon, "client-owner", owner);

    let service = AuthorizationService::new(team.clone());
    let descriptor = OperationDescriptor::new(
        "project_archive",
        ScopeKind::DirectProject,
        Some(Capability::ProjectConfigure),
    );
    let authority = daemon.request_authority_for_client("client-owner");
    let before = service
        .authorize(&AuthorizationRequest::new(
            authority.principal().clone(),
            descriptor,
            Some(project.clone()),
            "corr-1",
        ))
        .await;
    assert!(before.is_ok());

    team.revoke_membership(&project, &owner_id, membership.revision)
        .await
        .unwrap();
    let authority = daemon.request_authority_for_client("client-owner");
    let after = service
        .authorize(&AuthorizationRequest::new(
            authority.principal().clone(),
            descriptor,
            Some(project.clone()),
            "corr-2",
        ))
        .await;
    assert!(after.is_err());

    // The boundary agrees: the archived-project write now denies.
    let denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-revoked".to_owned(),
            CoreRequest::ProjectArchive {
                project_id: project.as_str().to_owned(),
            },
        ),
        "client-owner",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "authorization_denied");
}

#[tokio::test(flavor = "current_thread")]
async fn child_and_provider_authority_cannot_widen() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool);
    let service = AuthorizationService::new(team.clone());
    let project = ProjectId::new();

    // Child escalation fails closed at the contract.
    let parent = ProjectRole::Contributor.capabilities();
    let escalated = CapabilitySet::from_caps([Capability::MemberManage]);
    assert!(child_escalates(&parent, &escalated));
    assert!(authorize_child_delegation(&parent, &escalated).is_err());
    let narrowed =
        authorize_child_delegation(&parent, &CapabilitySet::from_caps([Capability::GitWrite]))
            .unwrap();
    assert!(narrowed.has(Capability::GitWrite));

    // Provider scope: personal is owner-only, project follows grants,
    // deployment needs an Owner grant.
    let alice = human_token(&team, "Alice", "client-alice").await;
    let bob = human_token(&team, "Bob", "client-bob").await;
    team.create_membership(&project, alice.principal_id(), ProjectRole::Contributor)
        .await
        .unwrap();
    let personal = ProviderScope::Personal {
        owner: alice.principal_id().clone(),
    };
    assert!(
        authorize_provider_use(&service, &alice, &personal, Capability::AgentInvoke, "c1")
            .await
            .is_ok()
    );
    assert!(
        authorize_provider_use(&service, &bob, &personal, Capability::AgentInvoke, "c1")
            .await
            .is_err()
    );
    let deployment = ProviderScope::deployment("deployment-1").unwrap();
    assert!(
        authorize_provider_use(&service, &bob, &deployment, Capability::AgentInvoke, "c1")
            .await
            .is_err()
    );
    // LocalOwner composes through the same API, never around it.
    let local = AuthenticatedPrincipal::local_owner("client-local");
    assert!(is_local_owner_broad(&local));
    assert!(!is_local_owner_broad(&alice));
    assert!(
        authorize_provider_use(&service, &local, &deployment, Capability::AgentInvoke, "c1")
            .await
            .is_ok()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_preserves_membership_and_attribution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("m003-restart.db");
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let pool = sqlx::SqlitePool::connect(&url).await.expect("connect");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    let team = TeamStore::new(pool.clone());
    let service = AuthorizationService::new(team.clone());
    let project = ProjectId::new();
    let ada = team
        .create_principal(PrincipalKind::Human, "Ada")
        .await
        .unwrap();
    let membership = team
        .create_membership(&project, &ada.id, ProjectRole::Maintainer)
        .await
        .unwrap();
    let bound = AuthenticatedPrincipal::internal_test(&ada, "client-ada");
    let descriptor = OperationDescriptor::new(
        "project_health",
        ScopeKind::DirectProject,
        Some(Capability::ProjectRead),
    );
    let decision = service
        .authorize(&AuthorizationRequest::new(
            bound.clone(),
            descriptor,
            Some(project.clone()),
            "corr-restart",
        ))
        .await
        .unwrap();
    let store = OriginAttributionStore::new(pool.clone());
    let attribution =
        codegg_core::authorization::OriginAttribution::from_authority(&bound, &decision);
    store
        .record("session", "session-restart", &attribution)
        .await
        .unwrap();
    pool.close().await;

    let pool2 = sqlx::SqlitePool::connect(&url).await.expect("reconnect");
    codegg_core::session::schema::migrate(&pool2)
        .await
        .expect("remigrate");
    let team2 = TeamStore::new(pool2.clone());
    let service2 = AuthorizationService::new(team2.clone());
    assert!(team2
        .has_capability(&project, &ada.id, Capability::ProjectConfigure)
        .await
        .unwrap());
    let decision2 = service2
        .authorize(&AuthorizationRequest::new(
            bound,
            descriptor,
            Some(project),
            "corr-restart-2",
        ))
        .await
        .unwrap();
    assert_eq!(decision2.membership_revision, Some(membership.revision));
    let store2 = OriginAttributionStore::new(pool2.clone());
    let reloaded = store2
        .get("session", "session-restart")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reloaded, attribution);
    pool2.close().await;
}
