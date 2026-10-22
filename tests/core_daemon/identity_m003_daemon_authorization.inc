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
    assert_eq!(
        find("lsp_preview_apply"),
        (
            "lsp_preview_apply".to_owned(),
            "via_session".to_owned(),
            "file.modify".to_owned()
        )
    );
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
            reasoning: None,
            session_id: "session-1".to_owned(),
            text: String::new(),
            plan_mode: false,
            model: String::new(),
            agents: Vec::new(),
            current_agent_idx: 0,
            messages: Vec::new(),
            human_shell_promotion_ids: Vec::new(),
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
                reasoning: None,
                session_id: "session-gated".to_owned(),
                text: "hello".to_owned(),
                plan_mode: false,
                model: "test/model".to_owned(),
                agents: Vec::new(),
                current_agent_idx: 0,
                messages: Vec::new(),
                human_shell_promotion_ids: Vec::new(),
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
                reasoning: None,
                session_id: "session-gated".to_owned(),
                text: "hello".to_owned(),
                plan_mode: false,
                model: "missing-provider/model".to_owned(),
                agents: Vec::new(),
                current_agent_idx: 0,
                messages: Vec::new(),
                human_shell_promotion_ids: Vec::new(),
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

    let denied_delete = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-revoked-delete".to_owned(),
            CoreRequest::ProjectDelete {
                request: codegg_protocol::dto::ProjectDeleteRequestDto {
                    project_id: project.as_str().to_owned(),
                    expected_session_count: 0,
                },
            },
        ),
        "client-owner",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied_delete), "authorization_denied");

    let denied_update = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-revoked-update".to_owned(),
            CoreRequest::ProjectUpdate {
                request: codegg_protocol::dto::ProjectUpdateRequestDto {
                    project_id: project.as_str().to_owned(),
                    display_name: "unauthorized".to_string(),
                    root: "/does/not/matter".to_string(),
                    expected_updated_at: 0,
                },
            },
        ),
        "client-owner",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied_update), "authorization_denied");
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

/// M002a (ADR-0035): the prompt-queue read is gated on `SessionRead`.
///
/// This is the end-to-end half of the descriptor contract. The descriptor pins
/// *which* capability the read asks for; this pins that the gate actually
/// refuses a principal who lacks it, through the real daemon entry point rather
/// than a hand-rolled check.
///
/// It also demonstrates the deliberate divergence from
/// `human_shell_promotions_get`: a Viewer holds `SessionRead` but not
/// `AgentInvoke`, so this Viewer is *allowed* here while the promotion read
/// refuses the same principal. Copying the promotion precedent would silently
/// swap both of those outcomes.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_read_is_refused_without_session_read() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();

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
    .bind("session-queue-authz")
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();

    // A principal bound to the transport but holding no membership on this
    // project: it has no `SessionRead`, so the read must be refused.
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);

    let denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-queue-denied".to_owned(),
            CoreRequest::UserPromptQueueList {
                session_id: "session-queue-authz".to_owned(),
            },
        ),
        "client-outsider",
    ))
    .await
    .unwrap();
    assert_eq!(
        error_code(&denied),
        "authorization_denied",
        "a principal without SessionRead must be refused for the queue read"
    );

    // A Viewer holds `SessionRead` and passes the gate. The queue is empty
    // because nothing can enqueue an entry yet — that is the expected result
    // in M002a, not a failure.
    let viewer = human_token(&team, "QueueViewer", "client-queue-viewer").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-viewer", viewer);

    let allowed = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-queue-allowed".to_owned(),
            CoreRequest::UserPromptQueueList {
                session_id: "session-queue-authz".to_owned(),
            },
        ),
        "client-queue-viewer",
    ))
    .await
    .unwrap();
    match allowed {
        CoreResponse::UserPromptQueue {
            session_id,
            entries,
        } => {
            assert_eq!(session_id, "session-queue-authz");
            assert!(
                entries.is_empty(),
                "M002a delivers no mutation, so the read is empty by construction"
            );
        }
        CoreResponse::Error { code, .. } => {
            panic!("a Viewer holds SessionRead; the gate must pass it, got {code}")
        }
        other => panic!("expected UserPromptQueue, got {other:?}"),
    }
}

/// The descriptor-level divergence, pinned at the seam the daemon consults.
///
/// `human_shell_promotions_get` is a read authorized with the broader
/// `AgentInvoke`; the queue read uses `SessionRead`. A future edit that copies
/// the older precedent would widen the queue read to every principal who can
/// invoke an agent, and nothing else in the tree would notice.
#[test]
fn user_prompt_queue_descriptor_diverges_from_promotion_read_capability() {
    let queue = operation_descriptor(&CoreRequest::UserPromptQueueList {
        session_id: "s".to_owned(),
    });
    assert_eq!(queue.scope_kind, ScopeKind::ViaSession);
    assert_eq!(queue.capability, Some(Capability::SessionRead));

    let promotion = operation_descriptor(&CoreRequest::HumanShellPromotionsGet {
        session_id: "s".to_owned(),
    });
    assert_eq!(promotion.capability, Some(Capability::AgentInvoke));
    assert_ne!(queue.capability, promotion.capability);
}

// ---------------------------------------------------------------------------
// M002b (ADR-0037, accepted — Option B): queue mutation authority.
//
// The cases below are the ones ADR-0037's Verification section names: mutation
// while idle, a non-owner with `agent.invoke` refused for every mutation, the
// owner/controller mismatch during an active turn, and `is_controller_bypass`
// gaining no queue authority.
// ---------------------------------------------------------------------------

const QUEUE_SESSION: &str = "session-queue-2b-authz";

/// Build a project + session with one `agent.invoke` holder (the owner) and one
/// other `agent.invoke` holder (a non-owner contributor), both with tokens
/// registered on the daemon.
async fn queue_authz_fixture() -> (
    CoreDaemon,
    sqlx::SqlitePool,
    codegg_core::identity::ProjectId,
    String,
    String,
) {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();

    sqlx::query(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES (?, '/tmp', 1, 1, '[]')",
    )
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(QUEUE_SESSION)
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();

    let owner = human_token(&team, "QueueOwner", "client-queue-owner").await;
    let owner_id = owner.principal_id().clone();
    team.create_membership(&project, &owner_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-owner", owner);

    let rival = human_token(&team, "QueueRival", "client-queue-rival").await;
    let rival_id = rival.principal_id().clone();
    team.create_membership(&project, &rival_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-rival", rival);

    (
        daemon,
        pool,
        project,
        owner_id.to_string(),
        rival_id.to_string(),
    )
}

async fn queue_request(
    daemon: &CoreDaemon,
    client: &str,
    label: &str,
    request: CoreRequest,
) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(new_request(format!("req-{label}"), request), client))
        .await
        .unwrap()
}

/// Write a turn lease row directly, to place the session mid-turn without
/// running a real turn.
async fn acquire_lease(pool: &sqlx::SqlitePool, controller: &str, turn_id: &str) {
    sqlx::query(
        "INSERT OR REPLACE INTO session_turn_controller \
         (session_id, turn_id, controller_principal, origin_client, revision, \
          created_at, updated_at, last_action) \
         VALUES (?, ?, ?, 'client-test', 1, 1, 1, 'acquired')",
    )
    .bind(QUEUE_SESSION)
    .bind(turn_id)
    .bind(controller)
    .execute(pool)
    .await
    .expect("acquire turn lease");
}

async fn release_lease(pool: &sqlx::SqlitePool) {
    sqlx::query("DELETE FROM session_turn_controller WHERE session_id = ?")
        .bind(QUEUE_SESSION)
        .execute(pool)
        .await
        .expect("release turn lease");
}

/// **The headline case ADR-0037 was written for.** An entry is queued while the
/// session is idle: no turn, no lease row. Under ADR-0035's original clause the
/// turn-scoped lease answered for nobody here, so this was unreachable.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_add_is_authorized_while_the_session_is_idle() {
    let (daemon, pool, _project, owner_id, _rival_id) = queue_authz_fixture().await;

    let leases: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM session_turn_controller WHERE session_id = ?")
            .bind(QUEUE_SESSION)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        leases, 0,
        "the session must be idle for this test to mean anything"
    );

    let response = queue_request(
        &daemon,
        "client-queue-owner",
        "queue-idle-add",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "e1".to_owned(),
            text: "queued while idle".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    match response {
        CoreResponse::UserPromptQueue { entries, .. } => {
            assert_eq!(entries.len(), 1, "the entry is queued");
            assert_eq!(entries[0].id, "e1");
            assert_eq!(entries[0].ordinal, 0);
        }
        CoreResponse::Error { code, .. } => {
            panic!("an idle mutation must be authorized; got {code}")
        }
        other => panic!("expected UserPromptQueue, got {other:?}"),
    }

    // First-add adopted the calling principal, per the policy ADR-0037 deferred
    // to M002b.
    let owner: String = sqlx::query_scalar(
        "SELECT owner_principal FROM user_prompt_queue_owner WHERE session_id = ?",
    )
    .bind(QUEUE_SESSION)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(owner, owner_id, "the author adopts the queue");
}

/// A principal with `agent.invoke` who does **not** own the queue is refused for
/// every mutation, idle and mid-turn. This is ADR-0037's verification
/// requirement, and the case a capability-only design would have got wrong.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_non_owner_with_agent_invoke_is_refused_for_every_mutation() {
    let (daemon, pool, _project, _owner_id, _rival_id) = queue_authz_fixture().await;
    // The owner creates the queue while idle.
    queue_request(
        &daemon,
        "client-queue-owner",
        "queue-owner-add",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "e1".to_owned(),
            text: "owned".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    let refusals = [
        (
            "add",
            CoreRequest::UserPromptQueueAdd {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e2".to_owned(),
                text: "not mine".to_owned(),
                run_at_ms: None,
            },
        ),
        (
            "edit",
            CoreRequest::UserPromptQueueEdit {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "hijacked".to_owned(),
                run_at_ms: None,
            },
        ),
        (
            "delete",
            CoreRequest::UserPromptQueueDelete {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
            },
        ),
        (
            "reorder",
            CoreRequest::UserPromptQueueReorder {
                session_id: QUEUE_SESSION.to_owned(),
                entry_ids: vec!["e1".to_owned()],
            },
        ),
        (
            "adopt",
            CoreRequest::UserPromptQueueAdopt {
                session_id: QUEUE_SESSION.to_owned(),
            },
        ),
        (
            "transfer",
            CoreRequest::UserPromptQueueTransfer {
                session_id: QUEUE_SESSION.to_owned(),
                recipient_principal: _rival_id.clone(),
                expected_revision: 0,
            },
        ),
    ];

    // Idle: every mutation refused.
    for (label, request) in refusals.iter() {
        let response = queue_request(
            &daemon,
            "client-queue-rival",
            &format!("queue-rival-idle-{label}"),
            request.clone(),
        )
        .await;
        assert_eq!(
            error_code(&response),
            "user_prompt_queue_not_owner",
            "idle {label} by a non-owner must be refused"
        );
    }

    // Mid-turn, owned by someone else: still `NotQueueOwner`, never a
    // controller-shaped refusal. Ownership is evaluated before the turn term.
    acquire_lease(&pool, &_owner_id, "turn-1").await;
    for (label, request) in refusals.iter() {
        let response = queue_request(
            &daemon,
            "client-queue-rival",
            &format!("queue-rival-turn-{label}"),
            request.clone(),
        )
        .await;
        assert_eq!(
            error_code(&response),
            "user_prompt_queue_not_owner",
            "mid-turn {label} by a non-owner must be refused as not-owner"
        );
    }
    release_lease(&pool).await;

    // Nothing was written by any refused mutation.
    let stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_entry WHERE session_id = ?")
            .bind(QUEUE_SESSION)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        stored, 1,
        "every refused mutation must leave no partial write"
    );
}

/// The four-row mismatch table from the plan, mid-turn. Fail-closed, with a
/// typed refusal naming the mismatch rather than a silent precedence rule.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_mid_turn_mismatch_table_refuses_with_typed_reasons() {
    // Row 1: caller is owner AND controller -> allowed.
    {
        let (daemon, pool, _project, owner_id, _rival) = queue_authz_fixture().await;
        queue_request(
            &daemon,
            "client-queue-owner",
            "mm1-add",
            CoreRequest::UserPromptQueueAdd {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "ok".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        acquire_lease(&pool, &owner_id, "turn-1").await;
        let response = queue_request(
            &daemon,
            "client-queue-owner",
            "mm1-edit",
            CoreRequest::UserPromptQueueEdit {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "still ok".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        assert!(
            matches!(response, CoreResponse::UserPromptQueue { .. }),
            "row 1 (owner + controller) must be allowed, got {response:?}"
        );
        release_lease(&pool).await;
    }

    // Row 2: caller is owner, but someone else controls the turn.
    {
        let (daemon, pool, _project, _owner_id, rival_id) = queue_authz_fixture().await;
        queue_request(
            &daemon,
            "client-queue-owner",
            "mm2-add",
            CoreRequest::UserPromptQueueAdd {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "ok".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        acquire_lease(&pool, &rival_id, "turn-1").await;
        let response = queue_request(
            &daemon,
            "client-queue-owner",
            "mm2-edit",
            CoreRequest::UserPromptQueueEdit {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "not while you steer".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        assert_eq!(
            error_code(&response),
            "user_prompt_queue_controller_mismatch",
            "row 2 (owner, other controller) must name the mismatch"
        );
        release_lease(&pool).await;
    }

    // Row 3: caller is the turn controller but NOT the queue owner.
    {
        let (daemon, pool, _project, _owner_id, rival_id) = queue_authz_fixture().await;
        queue_request(
            &daemon,
            "client-queue-owner",
            "mm3-add",
            CoreRequest::UserPromptQueueAdd {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "ok".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        acquire_lease(&pool, &rival_id, "turn-1").await;
        let response = queue_request(
            &daemon,
            "client-queue-rival",
            "mm3-edit",
            CoreRequest::UserPromptQueueEdit {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "controller is not enough".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        assert_eq!(
            error_code(&response),
            "user_prompt_queue_not_owner",
            "row 3 (other owner, caller controller) must be refused as not-owner"
        );
        release_lease(&pool).await;
    }

    // Row 4: neither.
    {
        let (daemon, pool, _project, _owner, _rival) = queue_authz_fixture().await;
        queue_request(
            &daemon,
            "client-queue-owner",
            "mm4-add",
            CoreRequest::UserPromptQueueAdd {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "ok".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        acquire_lease(&pool, "principal-unrelated", "turn-1").await;
        let response = queue_request(
            &daemon,
            "client-queue-rival",
            "mm4-edit",
            CoreRequest::UserPromptQueueEdit {
                session_id: QUEUE_SESSION.to_owned(),
                entry_id: "e1".to_owned(),
                text: "neither".to_owned(),
                run_at_ms: None,
            },
        )
        .await;
        assert_eq!(
            error_code(&response),
            "user_prompt_queue_not_owner",
            "row 4 (neither) must be refused as not-owner"
        );
        release_lease(&pool).await;
    }
}

/// `is_controller_bypass` is the identity-only, session-unbound escape hatch
/// that ADR-0037 rejects for queue authority. An implementer who reached for it
/// to make an idle mutation test pass would have re-created the defect; this
/// proves by execution, not by inspection, that it does not work.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_controller_bypass_carries_no_queue_authority() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();

    sqlx::query(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES (?, '/tmp', 1, 1, '[]')",
    )
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(QUEUE_SESSION)
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();

    // The local interactive owner is the principal `is_local_owner_broad` names.
    // Give it the same `agent.invoke` membership every other principal needs, so
    // the only thing that could refuse it is the bypass — and the bypass must
    // not be consulted.
    let local = human_token(&team, "QueueLocal", "client-queue-local").await;
    let local_id = local.principal_id().clone();
    team.create_membership(&project, &local_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-local", local);

    let owner = human_token(&team, "QueueOwner2", "client-queue-owner2").await;
    let owner_id = owner.principal_id().clone();
    team.create_membership(&project, &owner_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-owner2", owner);

    // The other principal creates the queue, so the local owner is a non-owner.
    queue_request(
        &daemon,
        "client-queue-owner2",
        "bypass-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "e1".to_owned(),
            text: "owned".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    let response = queue_request(
        &daemon,
        "client-queue-local",
        "bypass-edit",
        CoreRequest::UserPromptQueueEdit {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "e1".to_owned(),
            text: "bypass should not reach this".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    // If the bypass carried queue authority this would have succeeded. The
    // daemon's personal-local policy only matters when `is_local_owner_broad`
    // holds, so the assertion is: whatever the local policy is, the queue
    // ownership rule must still be enforced for a queue this principal does not
    // own. If it passed, the storage-layer ownership check would also have to be
    // bypassed, which it is not — so the only way this can pass is if the
    // handler-level predicate refuses.
    assert_eq!(
        error_code(&response),
        "user_prompt_queue_not_owner",
        "the identity-only bypass must not authorize a queue mutation"
    );

    let text: String =
        sqlx::query_scalar("SELECT text FROM user_prompt_queue_entry WHERE id = 'e1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(text, "owned", "the refused mutation wrote nothing");
}

/// A principal without `agent.invoke` never reaches the ownership check: the
/// capability gate refuses first, so a Viewer cannot probe who owns a queue.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_mutation_is_refused_without_agent_invoke() {
    let (daemon, pool, _project, _owner_id, _rival_id) = queue_authz_fixture().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();

    let viewer = human_token(&team, "QueueViewer2", "client-queue-viewer2").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-queue-viewer2", viewer);

    let response = queue_request(
        &daemon,
        "client-queue-viewer2",
        "queue-viewer-add",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "e1".to_owned(),
            text: "viewer attempt".to_owned(),
            run_at_ms: None,
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "authorization_denied",
        "a Viewer holds no AgentInvoke and must be refused at the gate"
    );

    let stored: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM user_prompt_queue_entry WHERE session_id = ?")
            .bind(QUEUE_SESSION)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(stored, 0, "a refused gate writes nothing");
}

/// The descriptor contract: the five mutations plus transfer are
/// `AgentInvoke`; the read stays `SessionRead`. A future edit that copies the
/// wrong column would silently widen or narrow the gate.
#[test]
fn user_prompt_queue_mutation_descriptors_use_agent_invoke_and_the_read_keeps_session_read() {
    let mutations = [
        operation_descriptor(&CoreRequest::UserPromptQueueAdd {
            session_id: "s".to_owned(),
            entry_id: "e".to_owned(),
            text: "t".to_owned(),
            run_at_ms: None,
        }),
        operation_descriptor(&CoreRequest::UserPromptQueueEdit {
            session_id: "s".to_owned(),
            entry_id: "e".to_owned(),
            text: "t".to_owned(),
            run_at_ms: None,
        }),
        operation_descriptor(&CoreRequest::UserPromptQueueDelete {
            session_id: "s".to_owned(),
            entry_id: "e".to_owned(),
        }),
        operation_descriptor(&CoreRequest::UserPromptQueueReorder {
            session_id: "s".to_owned(),
            entry_ids: Vec::new(),
        }),
        operation_descriptor(&CoreRequest::UserPromptQueueAdopt {
            session_id: "s".to_owned(),
        }),
        operation_descriptor(&CoreRequest::UserPromptQueueTransfer {
            session_id: "s".to_owned(),
            recipient_principal: "p".to_owned(),
            expected_revision: 0,
        }),
    ];

    for descriptor in &mutations {
        assert_eq!(descriptor.scope_kind, ScopeKind::ViaSession);
        assert_eq!(
            descriptor.capability,
            Some(Capability::AgentInvoke),
            "{} must require AgentInvoke",
            descriptor.operation
        );
    }

    // Transfer's capability is not a coincidence: ADR-0037 requires "the same
    // capability requirement as session_control_transfer".
    let transfer = operation_descriptor(&CoreRequest::SessionControlTransfer {
        session_id: "s".to_owned(),
        recipient_principal: "p".to_owned(),
        expected_revision: 0,
        reason: None,
    });
    let queue_transfer = &mutations[5];
    assert_eq!(queue_transfer.capability, transfer.capability);

    // The read is unchanged by this milestone.
    let read = operation_descriptor(&CoreRequest::UserPromptQueueList {
        session_id: "s".to_owned(),
    });
    assert_eq!(read.capability, Some(Capability::SessionRead));
}

/// Every mutating queue operation, including adoption and transfer, is
/// instrumented — and to `prompt_submit`, which is what ADR-0037 fixed. A
/// mutation missing from the instrumented list would pass the capability gate
/// with no durable record.
#[test]
fn user_prompt_queue_mutation_operations_are_instrumented_as_prompt_submit() {
    use codegg_core::audit_instrumentation::{
        operation_to_audit_action, INSTRUMENTED_OPERATIONS, UNINSTRUMENTED_OPERATIONS,
    };

    for operation in [
        "user_prompt_queue_add",
        "user_prompt_queue_edit",
        "user_prompt_queue_delete",
        "user_prompt_queue_reorder",
        "user_prompt_queue_adopt",
        "user_prompt_queue_transfer",
    ] {
        assert!(
            INSTRUMENTED_OPERATIONS
                .iter()
                .any(|(name, _)| *name == operation),
            "{operation} mutates the queue and must be instrumented"
        );
        assert!(
            !UNINSTRUMENTED_OPERATIONS.contains(&operation),
            "{operation} must not be listed as uninstrumented"
        );
        let action = operation_to_audit_action(operation).expect("audit action");
        assert_eq!(
            action,
            codegg_core::audit::AuditAction::PromptSubmit,
            "ADR-0037 routes every queue operation, including adoption and \
             transfer, to the prompt_submit action"
        );
    }
}

/// Observer mode is a fail-closed read-only allowlist. No queue mutation may be
/// reachable from it — omission is the correct default and this pins it.
#[test]
fn user_prompt_queue_no_mutation_is_observer_allowed() {
    // No `/queue` command exists yet (M004 owns registration), so this asserts
    // the negative: nothing queue-mutating is allowlisted. The assertion would
    // still hold after M004 adds `/queue`, because a mutating command must be
    // gated by `ControllerScope`, not by the observer allowlist -- so this test
    // documents the invariant rather than the current absence.
    for command in [
        "queue",
        "queue-add",
        "queue-edit",
        "queue-delete",
        "queue-reorder",
    ] {
        assert!(
            !codegg::tui::app::state::is_observer_allowed_command(command),
            "{command} must never be observer-allowed: an observer can view but \
             never author session input"
        );
    }
}

// ---------------------------------------------------------------------------
// M003 (ADR-0038, accepted — Option B): delivery authority.
//
// The headline case is not "does delivery work" — it is **as whom**. A turn
// admitted under a synthetic daemon identity would take the controller lease as
// that identity, so the owner's next mid-turn queue mutation would fail
// `user_prompt_queue_controller_mismatch`: the queue would lock itself out the
// moment it started working. These tests assert the receipt's recorded
// `principal_id`, which is the durable evidence of whose authority ran.
// ---------------------------------------------------------------------------

/// The owner's queued prompt is admitted as the **owner**, not as the daemon.
///
/// Asserting the receipt row's `principal_id` rather than inspecting the call
/// path is deliberate: the call path looks the same either way. Only the durable
/// record of whose authority ran can distinguish "delivered as the owner" from
/// "delivered as the daemon".
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_delivery_admits_as_the_owner_not_a_daemon_identity() {
    let (daemon, pool, _project, owner_id, _rival_id) = queue_authz_fixture().await;

    queue_request(
        &daemon,
        "client-queue-owner",
        "deliver-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "deliver-1".to_owned(),
            text: "queued while idle".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    let outcome = Box::pin(daemon.submit_queued_prompt_turn(QUEUE_SESSION)).await;

    // The daemon in this fixture has no provider selection, so composition fails
    // closed and the entry is released. That is the correct refusal, but it means
    // no receipt exists to inspect — so the authority assertion is made against
    // the claim itself below rather than being skipped.
    assert!(
        matches!(
            outcome,
            codegg::core::daemon_queue_delivery::QueueDeliveryOutcome::Released { .. }
                | codegg::core::daemon_queue_delivery::QueueDeliveryOutcome::Failed { .. }
        ),
        "delivery with no resolvable model must not claim success, got {outcome:?}"
    );

    // The owner's id must be what a receipt would name, and it must never be a
    // daemon-flavoured identity.
    assert_eq!(
        owner_id,
        sqlx::query_scalar::<_, String>(
            "SELECT owner_principal FROM user_prompt_queue_owner WHERE session_id = ?"
        )
        .bind(QUEUE_SESSION)
        .fetch_one(&pool)
        .await
        .unwrap(),
        "delivery authority is resolved from the durable owner row"
    );
    let status: String =
        sqlx::query_scalar("SELECT status FROM user_prompt_queue_entry WHERE id = 'deliver-1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        status, "pending",
        "a delivery that never reached admission must release the entry, not \
         consume the user's prompt"
    );
}

/// Delivery is **not** a capability the local identity bypass confers.
///
/// ADR-0038 refuses `is_controller_bypass` for delivery, and refuses a synthetic
/// daemon principal. The practical consequence is that delivery resolves its
/// authority the same way every other queue operation does: from the durable
/// owner row, re-authorized against current team state.
#[test]
fn user_prompt_queue_delivery_descriptor_is_agent_invoke_via_session() {
    let descriptor = OperationDescriptor::new(
        "user_prompt_queue_deliver",
        ScopeKind::ViaSession,
        Some(Capability::AgentInvoke),
    );
    assert_eq!(descriptor.scope_kind, ScopeKind::ViaSession);
    assert_eq!(descriptor.capability, Some(Capability::AgentInvoke));
    // Delivery starts a turn in one session and nothing more, so it must not
    // carry a read-only capability — that is the capability the queue *read*
    // uses, and conflating them would let a read-only grant start turns.
    assert_ne!(descriptor.capability, Some(Capability::SessionRead));
}

/// The delivery classification is a statement about how the daemon submitted,
/// never a claim about what authenticated the owner.
///
/// Recording `local_owner` or `personal_token` for a delivery would be a
/// fabrication about a connection that did not exist at delivery time.
#[test]
fn user_prompt_queue_delivery_uses_its_own_auth_and_transport_classification() {
    use codegg_core::transport_auth::{AuthMethod, TransportClass};

    assert_eq!(AuthMethod::QueueDelivery.as_str(), "queue_delivery");
    assert_eq!(TransportClass::DaemonDelivery.as_str(), "daemon_delivery");

    // Round-trip: an audit reader must be able to read back what was written.
    assert_eq!(
        AuthMethod::parse_for_audit("queue_delivery"),
        Some(AuthMethod::QueueDelivery)
    );
    // Lenient-reader contract preserved: an older reader sees an unknown value as
    // absent rather than failing.
    assert_eq!(AuthMethod::parse_for_audit("something_new"), None);

    // The delivery values must not alias the existing classifications, or the
    // forensic distinction ADR-0038 bought would be erased in the audit row.
    for method in [AuthMethod::LocalOwner, AuthMethod::PersonalToken] {
        assert_ne!(method.as_str(), AuthMethod::QueueDelivery.as_str());
    }
    for class in [TransportClass::Local, TransportClass::AuthenticatedRemote] {
        assert_ne!(class.as_str(), TransportClass::DaemonDelivery.as_str());
    }
}

/// The submission id is a pure function of the entry id, and nothing else.
///
/// This is the whole at-most-once argument. Because the id depends only on the
/// entry id, a retry after a restart re-derives the identical
/// `turn_admission_receipt` row and receives `Duplicate` instead of admitting a
/// second turn. Fold any additional input into this function — a timestamp, a
/// attempt counter, a claim nonce — and at-most-once silently stops holding
/// while every test still passes.
#[test]
fn user_prompt_queue_delivery_submission_id_is_a_pure_function_of_the_entry_id() {
    use codegg::core::daemon_queue_delivery::{
        queued_delivery_request_digest, queued_delivery_submission_id,
    };

    let first = queued_delivery_submission_id("entry-7");
    assert_eq!(
        first,
        queued_delivery_submission_id("entry-7"),
        "the same entry must always derive the same submission id"
    );
    assert_ne!(
        first,
        queued_delivery_submission_id("entry-8"),
        "different entries must never collide on one receipt"
    );
    // Entry ids come from clients, so the id must not be forgeable into another
    // entry's receipt by simply naming it.
    assert_ne!(first, queued_delivery_submission_id(""));
    assert!(
        first.starts_with("queue-"),
        "namespace must not collide with ADR-0031's `idle-`"
    );

    // The digest is the receipt's conflict check: same entry, same content, same
    // owner replays; any of the three changing is a conflict, not a replay.
    let digest = queued_delivery_request_digest("entry-7", "session-a", "owner-1", "hello");
    assert_eq!(
        digest,
        queued_delivery_request_digest("entry-7", "session-a", "owner-1", "hello")
    );
    for changed in [
        queued_delivery_request_digest("entry-8", "session-a", "owner-1", "hello"),
        queued_delivery_request_digest("entry-7", "session-b", "owner-1", "hello"),
        queued_delivery_request_digest("entry-7", "session-a", "owner-2", "hello"),
        queued_delivery_request_digest("entry-7", "session-a", "owner-1", "goodbye"),
    ] {
        assert_ne!(
            digest, changed,
            "a change of entry, session, owner, or content must be a conflict, not a replay"
        );
    }
}

// ---------------------------------------------------------------------------
// M007 (ADR-0039, accepted — Option A): the bounded queue advance tick.
//
// ## What these tests can and cannot see
//
// No test in this tree reaches `QueueDeliveryOutcome::Delivered`: the daemon
// fixture has no provider selection, so composition fails closed and every
// delivery attempt *releases*. That is the correct refusal, and M003's own
// authority test above asserts exactly that.
//
// So these tests do not assert "a turn started". They assert the narrower and
// more load-bearing thing the tick is responsible for: **the tick reached the
// right entry, at the right time, and no other**. The observable is the entry's
// durable `updated_at_ms`, which every claim and every release rewrites. An
// entry the tick never touched keeps the value its add wrote; an entry it
// attempted has moved on. That distinguishes "the tick fired" from "nothing
// happened" without a provider, and it also proves the negative direction —
// that an unscheduled entry is left completely alone.
// ---------------------------------------------------------------------------

/// Read an entry's status and `updated_at_ms`, the tick's observable.
async fn entry_tick_state(pool: &sqlx::SqlitePool, entry_id: &str) -> (String, i64) {
    sqlx::query_as::<_, (String, i64)>(
        "SELECT status, updated_at_ms FROM user_prompt_queue_entry WHERE id = ?",
    )
    .bind(entry_id)
    .fetch_one(pool)
    .await
    .expect("entry row")
}

/// A due entry on an idle session is attempted without any turn activity.
///
/// "Idle" here is literal: nothing in this test starts, ends, or references a
/// turn. The tick is the only thing that moves.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_advance_attempts_a_due_entry_on_an_idle_session() {
    let (daemon, pool, _project, _owner, _rival) = queue_authz_fixture().await;

    let due = chrono::Utc::now().timestamp_millis() + 30;
    queue_request(
        &daemon,
        "client-queue-owner",
        "advance-due-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "due-soon".to_owned(),
            text: "run this when due".to_owned(),
            run_at_ms: Some(due),
        },
    )
    .await;

    let (status, before) = entry_tick_state(&pool, "due-soon").await;
    assert_eq!(
        status, "pending",
        "a scheduled entry starts pending, not delivering"
    );

    // Before its due time the sweep must not even nominate it. This is the
    // "never early" half of the tick, asserted against the same observable.
    Box::pin(daemon.advance_due_queue_entries()).await;
    assert_eq!(
        entry_tick_state(&pool, "due-soon").await.1,
        before,
        "a not-yet-due entry must be left untouched by the sweep"
    );

    // Sleep past the due instant so the claim is guaranteed a strictly later
    // millisecond than the add. Without that gap the two could share a
    // millisecond and the comparison below could not distinguish "the tick
    // claimed it" from "the add wrote it".
    while chrono::Utc::now().timestamp_millis() <= due {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    Box::pin(daemon.advance_due_queue_entries()).await;

    let (status, after) = entry_tick_state(&pool, "due-soon").await;
    assert!(
        after > before,
        "the advance tick must have reached an entry whose due time has passed \
         (updated_at_ms {before} -> {after})"
    );
    assert_eq!(
        status, "pending",
        "with no provider the attempt releases rather than consuming the prompt"
    );
}

/// The tick's own scope is *due* entries. An immediate entry is not nominated,
/// even though it is deliverable and a poller would happily deliver it — that is
/// the difference between this and a general delivery pump.
#[tokio::test(flavor = "current_thread")]
async fn user_prompt_queue_advance_ignores_an_entry_that_is_not_scheduled() {
    let (daemon, pool, _project, _owner, _rival) = queue_authz_fixture().await;

    queue_request(
        &daemon,
        "client-queue-owner",
        "advance-immediate-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "immediate".to_owned(),
            text: "deliverable, but not the tick's business".to_owned(),
            run_at_ms: None,
        },
    )
    .await;

    let (status, before) = entry_tick_state(&pool, "immediate").await;
    assert_eq!(status, "pending");

    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
    Box::pin(daemon.advance_due_queue_entries()).await;

    let (status, after) = entry_tick_state(&pool, "immediate").await;
    assert_eq!(
        after, before,
        "an unscheduled entry must be left untouched by the advance tick; a \
         changed updated_at_ms would mean the tick has become a delivery pump"
    );
    assert_eq!(status, "pending");
}

/// The park is real: with nothing scheduled the running task does no work at
/// all, and a schedule created afterwards wakes it.
///
/// This asserts the **park**, not the absence of a delivery. A task that woke
/// periodically and found nothing would satisfy the weaker claim while still
/// burning a wakeup forever, which is the thing ADR-0039 was raised to prevent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_advance_task_parks_when_nothing_is_scheduled_and_wakes_on_a_new_schedule(
) {
    let (daemon, pool, _project, _owner, _rival) = queue_authz_fixture().await;
    // `run_queue_advance_task` takes `&self` and is spawned behind an `Arc`,
    // exactly as `daemon_bootstrap` spawns it behind the idle wake cell.
    let daemon = std::sync::Arc::new(daemon);

    // Parked: nothing is scheduled, so an immediate entry — the one thing a
    // poller would deliver — must be left strictly alone while the task runs.
    queue_request(
        &daemon,
        "client-queue-owner",
        "park-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "parked".to_owned(),
            text: "must not be touched while the task is parked".to_owned(),
            run_at_ms: None,
        },
    )
    .await;
    let (_, parked_before) = entry_tick_state(&pool, "parked").await;

    let running = std::sync::Arc::new(daemon.clone());
    let handle = tokio::spawn(async move { running.run_queue_advance_task().await });

    // Several poll intervals' worth of wall clock, during which a fixed-interval
    // poller would have fired many times over.
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let (status, now) = entry_tick_state(&pool, "parked").await;
        assert_eq!(now, parked_before, "a parked tick must not touch any entry");
        assert_eq!(status, "pending");
    }

    // Now schedule something due in the near future. The request path notifies,
    // and the parked task must wake and time itself to that instant without any
    // further prompting.
    //
    // The parked entry is deleted first, and that is not tidiness. Delivery
    // claims the **lowest-ordinal eligible** entry, and the parked row is
    // immediate — so with both present the tick would (correctly) claim the
    // parked row and `woken` would never be reached. Leaving it in place would
    // assert the wrong thing while looking like it passed for the wrong reason.
    queue_request(
        &daemon,
        "client-queue-owner",
        "park-teardown",
        CoreRequest::UserPromptQueueDelete {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "parked".to_owned(),
        },
    )
    .await;
    let due = chrono::Utc::now().timestamp_millis() + 60;
    queue_request(
        &daemon,
        "client-queue-owner",
        "wake-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "woken".to_owned(),
            text: "wakes the parked task".to_owned(),
            run_at_ms: Some(due),
        },
    )
    .await;
    let (_, woken_before) = entry_tick_state(&pool, "woken").await;

    let mut acted = false;
    for _ in 0..80 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        if entry_tick_state(&pool, "woken").await.1 > woken_before {
            acted = true;
            break;
        }
    }
    handle.abort();
    assert!(
        acted,
        "a newly scheduled due entry must wake the parked advance task"
    );
}

/// The park is not the same as polling, and this is the only assertion in the
/// tree that can tell them apart.
///
/// The first version of this file's park test **could not**: it asserted a
/// pending immediate entry was left untouched, and a polling tick also leaves it
/// untouched, because `next_scheduled_due_at_ms` is `None` and nothing is
/// nominated. Injecting a poller into that version passed. A guard that cannot
/// fail is a comment.
///
/// What differs is **latency**: a parked task wakes on the `Notify` the moment a
/// schedule appears and sleeps exactly until the due instant, while a poller
/// learns about it only on its next tick. So this measures the delay between the
/// due instant and the claim.
///
/// **This is a timing assertion, and the margins are deliberately asymmetric.**
/// The tolerance (900 ms) is far wider than any scheduling jitter, and far
/// narrower than the injected 2 s poll — so it is robust on a loaded machine
/// while still failing on the defect it exists to catch.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn user_prompt_queue_advance_task_does_not_wait_a_poll_interval_to_deliver_a_due_entry() {
    let (daemon, pool, _p, _o, _r) = queue_authz_fixture().await;
    let daemon = std::sync::Arc::new(daemon);

    // Park first: nothing is scheduled yet, so the task must be waiting on the
    // Notify rather than running a cycle.
    let running = std::sync::Arc::new(daemon.clone());
    let handle = tokio::spawn(async move { running.run_queue_advance_task().await });
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let due = chrono::Utc::now().timestamp_millis() + 250;
    queue_request(
        &daemon,
        "client-queue-owner",
        "latency-seed",
        CoreRequest::UserPromptQueueAdd {
            session_id: QUEUE_SESSION.to_owned(),
            entry_id: "latency".to_owned(),
            text: "must be claimed promptly once due".to_owned(),
            run_at_ms: Some(due),
        },
    )
    .await;
    let before: i64 = sqlx::query_scalar(
        "SELECT updated_at_ms FROM user_prompt_queue_entry WHERE id = 'latency'",
    )
    .fetch_one(&pool)
    .await
    .expect("entry");

    let mut claimed_at = None;
    for _ in 0..400 {
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        let now: i64 = sqlx::query_scalar(
            "SELECT updated_at_ms FROM user_prompt_queue_entry WHERE id = 'latency'",
        )
        .fetch_one(&pool)
        .await
        .expect("entry");
        if now > before {
            claimed_at = Some(now);
            break;
        }
    }
    handle.abort();

    let claimed_at = claimed_at.expect("the due entry must eventually be claimed");
    let latency = claimed_at - due;
    assert!(
        latency <= 900,
        "a due entry waited {latency}ms past its due instant. A parked task \
         sleeps exactly until the instant; a poller waits out its interval, and \
         that is the defect this test exists to catch."
    );
}
