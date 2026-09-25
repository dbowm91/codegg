//! Team Collaboration Corrective M004 — shared-session controller lease.
//!
//! Boundary proof for ADR-0007: the principal that successfully submits
//! a turn controls steer/cancel and permission/question responses for
//! that turn until terminal state or an explicit authorized
//! transfer/takeover. Observers and chat grants never confer control,
//! disconnect never transfers control, and every transition is
//! attributable and revisioned.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::session_control::SessionControllerStore;
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

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
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

/// Second device for an existing principal (same principal id, new client binding).
async fn second_device(
    team: &TeamStore,
    principal: &PrincipalId,
    client_id: &str,
) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let (plaintext, _) = tokens
        .create_personal_token(principal, "device-2", None)
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

async fn seed_session(pool: &sqlx::SqlitePool, project: &ProjectId, session_id: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES (?, '/tmp', 1, 1, '[]')",
    )
    .bind(project.as_str())
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .bind(project.as_str())
    .execute(pool)
    .await
    .unwrap();
}

struct Fixture {
    daemon: CoreDaemon,
    team: TeamStore,
    project: ProjectId,
    session_id: String,
    turn_id: String,
    /// Kept alive so steer/cancel channels stay open for the turn.
    _cancel_rx: tokio::sync::watch::Receiver<bool>,
    _steer_rx: tokio::sync::mpsc::Receiver<String>,
}

/// Seeded daemon with one session, one manually installed active turn,
/// and a durable lease held by Alice (Contributor). Bob, Cara
/// (Maintainer), Omar (Owner), and Victor (Viewer) hold memberships;
/// `client-alice-2` is Alice's second device.
async fn lease_fixture(session_tag: &str) -> Fixture {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let session_id = format!("sess-m004-{session_tag}");
    seed_session(team.pool(), &project, &session_id).await;

    let alice = human_token(&team, &format!("Alice-{session_tag}"), "client-alice").await;
    let alice_principal = team
        .get_principal(alice.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &alice_principal.id, ProjectRole::Contributor)
        .await
        .unwrap();
    let alice2 = second_device(&team, &alice_principal.id, "client-alice-2").await;

    let bob = human_token(&team, &format!("Bob-{session_tag}"), "client-bob").await;
    let bob_principal = team
        .get_principal(bob.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &bob_principal.id, ProjectRole::Contributor)
        .await
        .unwrap();

    let cara = human_token(&team, &format!("Cara-{session_tag}"), "client-cara").await;
    let cara_principal = team
        .get_principal(cara.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &cara_principal.id, ProjectRole::Maintainer)
        .await
        .unwrap();

    let omar = human_token(&team, &format!("Omar-{session_tag}"), "client-omar").await;
    let omar_principal = team
        .get_principal(omar.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &omar_principal.id, ProjectRole::Owner)
        .await
        .unwrap();

    let victor = human_token(&team, &format!("Victor-{session_tag}"), "client-victor").await;
    let victor_principal = team
        .get_principal(victor.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &victor_principal.id, ProjectRole::Viewer)
        .await
        .unwrap();

    register_client(&daemon, "client-alice", alice);
    register_client(&daemon, "client-alice-2", alice2);
    register_client(&daemon, "client-bob", bob);
    register_client(&daemon, "client-cara", cara);
    register_client(&daemon, "client-omar", omar);
    register_client(&daemon, "client-victor", victor);

    // In-memory runtime plus steer/cancel channels for the turn.
    let runtime = daemon.sessions.get_or_create(
        &session_id,
        codegg_core::workspace::WorkspaceId::new_unchecked(format!("ws-{session_tag}")),
        std::path::PathBuf::from("/tmp"),
        project.as_str().to_owned(),
        std::path::PathBuf::from("/tmp"),
    );
    let turn_id = format!("turn-m004-{session_tag}");
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let (steer_tx, steer_rx) = tokio::sync::mpsc::channel(32);
    {
        let mut active = runtime.active_turn.write().await;
        *active = Some(codegg::core::session_runtime::TurnHandle {
            turn_id: turn_id.clone(),
            cancel_tx,
            steer_tx: Some(steer_tx),
            started_at: chrono::Utc::now(),
            asset_pin: None,
            controller_principal: Some(alice_principal.id.as_str().to_owned()),
            controller_client: Some("client-alice".to_string()),
            controller_revision: 1,
        });
    }
    // Durable lease held by Alice.
    let store = SessionControllerStore::new(team.pool().clone());
    store
        .acquire(
            &session_id,
            &turn_id,
            &alice_principal.id,
            Some("client-alice"),
            1,
        )
        .await
        .unwrap();

    Fixture {
        daemon,
        team,
        project,
        session_id,
        turn_id,
        _cancel_rx: cancel_rx,
        _steer_rx: steer_rx,
    }
}

async fn steer_as(daemon: &CoreDaemon, client: &str, session: &str, turn: &str) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-steer-{client}-{}", uuid::Uuid::new_v4()),
            CoreRequest::TurnSteer {
                session_id: session.to_owned(),
                turn_id: turn.to_owned(),
                text: "redirect".to_owned(),
            },
        ),
        client,
    ))
    .await
    .unwrap()
}

async fn cancel_as(daemon: &CoreDaemon, client: &str, session: &str, turn: &str) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-cancel-{client}-{}", uuid::Uuid::new_v4()),
            CoreRequest::TurnCancel {
                session_id: session.to_owned(),
                turn_id: turn.to_owned(),
            },
        ),
        client,
    ))
    .await
    .unwrap()
}

// ── Authorization matrix ─────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn matrix_classifies_session_control_surface() {
    let matrix = codegg_core::authorization::operation_capability_matrix();
    let find = |operation: &str| {
        matrix
            .iter()
            .find(|(op, _, _)| op == operation)
            .unwrap_or_else(|| panic!("missing {operation}"))
            .clone()
    };
    assert_eq!(
        find("session_control_get"),
        (
            "session_control_get".to_owned(),
            "via_session".to_owned(),
            "session.read".to_owned()
        )
    );
    assert_eq!(find("session_control_request").2, "agent.invoke");
    assert_eq!(find("session_control_transfer").2, "agent.invoke");
    assert_eq!(find("session_control_release").2, "agent.invoke");
    assert_eq!(
        find("session_control_takeover"),
        (
            "session_control_takeover".to_owned(),
            "via_session".to_owned(),
            "project.configure".to_owned()
        )
    );
    // Existing turn rows are unchanged: the lease narrows, never widens.
    assert_eq!(find("turn_submit").2, "agent.invoke");
    assert_eq!(find("turn_steer").2, "agent.invoke");
    assert_eq!(find("turn_cancel").2, "agent.invoke");
}

// ── Steer/cancel gating ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn second_contributor_cannot_steer_or_cancel() {
    let fixture = lease_fixture("deny").await;
    assert_eq!(
        error_code(
            &steer_as(
                &fixture.daemon,
                "client-bob",
                &fixture.session_id,
                &fixture.turn_id
            )
            .await
        ),
        "session_control_not_controller"
    );
    assert_eq!(
        error_code(
            &cancel_as(
                &fixture.daemon,
                "client-bob",
                &fixture.session_id,
                &fixture.turn_id
            )
            .await
        ),
        "session_control_not_controller"
    );
    // The lease is untouched by the denials.
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    let record = store.get(&fixture.session_id).await.unwrap().unwrap();
    assert_eq!(record.revision, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn controller_can_steer_and_cancel() {
    let fixture = lease_fixture("allow").await;
    assert!(matches!(
        steer_as(
            &fixture.daemon,
            "client-alice",
            &fixture.session_id,
            &fixture.turn_id
        )
        .await,
        CoreResponse::Ack
    ));
    assert!(matches!(
        cancel_as(
            &fixture.daemon,
            "client-alice",
            &fixture.session_id,
            &fixture.turn_id
        )
        .await,
        CoreResponse::Ack
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn same_principal_second_device_resumes_control() {
    let fixture = lease_fixture("resume").await;
    // Client identity is audit/presence metadata only: Alice's second
    // device carries the same principal and passes the predicate.
    assert!(matches!(
        steer_as(
            &fixture.daemon,
            "client-alice-2",
            &fixture.session_id,
            &fixture.turn_id
        )
        .await,
        CoreResponse::Ack
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn viewer_and_outsider_cannot_control() {
    let fixture = lease_fixture("viewer").await;
    // Viewer lacks `agent.invoke`: the gate denies before the lease is
    // even consulted.
    let denied = steer_as(
        &fixture.daemon,
        "client-victor",
        &fixture.session_id,
        &fixture.turn_id,
    )
    .await;
    assert!(matches!(denied, CoreResponse::Error { .. }));
    // Outsider (no membership at all) is denied at the gate with the
    // typed denial (turn operations keep their capability shape; only
    // single-project reads map to not-found).
    let outsider = human_token(&fixture.team, "Outsider-viewer", "client-outsider").await;
    register_client(&fixture.daemon, "client-outsider", outsider);
    assert_eq!(
        error_code(
            &steer_as(
                &fixture.daemon,
                "client-outsider",
                &fixture.session_id,
                &fixture.turn_id
            )
            .await
        ),
        "authorization_denied"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn viewer_with_chat_grant_still_cannot_steer() {
    let fixture = lease_fixture("chat").await;
    // Owner grants the Viewer project chat (ADR-0006 overlay).
    let grant = Box::pin(
        fixture.daemon.handle_request_for_client(
            new_request(
                format!("req-chat-grant-{}", uuid::Uuid::new_v4()),
                CoreRequest::ChatProjectPolicySet {
                    project_id: fixture.project.as_str().to_owned(),
                    principal_id: fixture
                        .team
                        .get_principal(
                            &fixture
                                .daemon
                                .request_authority_for_client("client-victor")
                                .principal_id()
                                .clone(),
                        )
                        .await
                        .unwrap()
                        .unwrap()
                        .id
                        .as_str()
                        .to_owned(),
                    decision: Some(codegg::protocol::core::ChatPolicyDecisionDto::Allow),
                    expected_revision: None,
                },
            ),
            "client-omar",
        ),
    )
    .await
    .unwrap();
    assert!(
        matches!(grant, CoreResponse::ChatPolicy { .. }),
        "grant failed: {grant:?}"
    );
    // Chat access is orthogonal: steering still denies at the gate
    // because the Viewer holds no `agent.invoke`.
    let denied = steer_as(
        &fixture.daemon,
        "client-victor",
        &fixture.session_id,
        &fixture.turn_id,
    )
    .await;
    assert!(matches!(denied, CoreResponse::Error { .. }));
}

// ── Permission/question gating ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn second_contributor_cannot_answer_control_items() {
    let fixture = lease_fixture("perm").await;
    let perm_id = format!("perm-{}", uuid::Uuid::new_v4().simple());
    let (_tx, rx) = tokio::sync::oneshot::channel();
    // Keep the receiver alive: dropping it would fail the send and
    // mask the authorization outcome.
    let _rx = rx;
    codegg::bus::PermissionRegistry::register_with_session(
        fixture.session_id.clone(),
        Some(fixture.turn_id.clone()),
        perm_id.clone(),
        _tx,
    );
    // Bob is authorized on the session but is not the controller.
    let denied = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-perm-bob-{}", uuid::Uuid::new_v4()),
            CoreRequest::PermissionRespond {
                id: format!(
                    "perm:{}:{}:{}",
                    fixture.session_id, fixture.turn_id, perm_id
                ),
                choice: "allow".to_string(),
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "session_control_not_controller");
    // Alice answers the same item.
    let ok = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-perm-alice-{}", uuid::Uuid::new_v4()),
            CoreRequest::PermissionRespond {
                id: format!(
                    "perm:{}:{}:{}",
                    fixture.session_id, fixture.turn_id, perm_id
                ),
                choice: "allow".to_string(),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert!(
        matches!(ok, CoreResponse::Ack),
        "controller answer failed: {ok:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn second_contributor_cannot_answer_questions() {
    let fixture = lease_fixture("question").await;
    let question_id = format!("q-{}", uuid::Uuid::new_v4().simple());
    let (tx, _rx) = tokio::sync::oneshot::channel();
    codegg::bus::QuestionRegistry::register_with_session(
        fixture.session_id.clone(),
        Some(fixture.turn_id.clone()),
        question_id.clone(),
        tx,
    );
    let denied = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-q-bob-{}", uuid::Uuid::new_v4()),
            CoreRequest::QuestionRespond {
                id: format!(
                    "question:{}:{}:{}",
                    fixture.session_id, fixture.turn_id, question_id
                ),
                answers: serde_json::json!(["yes"]),
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "session_control_not_controller");
    codegg::bus::QuestionRegistry::unregister_scoped(&fixture.session_id, &question_id);
}

// ── Transfer / release / takeover ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn controller_transfer_hands_off_control() {
    let fixture = lease_fixture("transfer").await;
    let bob_principal = fixture
        .team
        .get_principal(
            &fixture
                .daemon
                .request_authority_for_client("client-bob")
                .principal_id()
                .clone(),
        )
        .await
        .unwrap()
        .unwrap();
    let response = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-transfer-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTransfer {
                session_id: fixture.session_id.clone(),
                recipient_principal: bob_principal.id.as_str().to_owned(),
                expected_revision: 1,
                reason: Some("handoff for lunch".to_string()),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::SessionControlUpdated { controller } => {
            let dto = controller.expect("transfer returns a lease");
            assert_eq!(dto.revision, 2);
            assert_eq!(dto.controller_principal, bob_principal.id.as_str());
        }
        other => panic!("expected transfer ack, got {other:?}"),
    }
    // Bob steers; Alice no longer can.
    assert!(matches!(
        steer_as(
            &fixture.daemon,
            "client-bob",
            &fixture.session_id,
            &fixture.turn_id
        )
        .await,
        CoreResponse::Ack
    ));
    assert_eq!(
        error_code(
            &steer_as(
                &fixture.daemon,
                "client-alice",
                &fixture.session_id,
                &fixture.turn_id
            )
            .await
        ),
        "session_control_not_controller"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unauthorized_transfer_fails_without_side_effect() {
    let fixture = lease_fixture("transfer-deny").await;
    let alice_principal = fixture
        .daemon
        .request_authority_for_client("client-alice")
        .principal_id()
        .clone();
    // Bob is not the controller and cannot move the lease.
    let denied = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-transfer-bob-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTransfer {
                session_id: fixture.session_id.clone(),
                recipient_principal: alice_principal.as_str().to_owned(),
                expected_revision: 1,
                reason: None,
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "session_control_not_controller");
    // Stale revisions conflict without overwrite.
    let bob_principal = fixture
        .daemon
        .request_authority_for_client("client-bob")
        .principal_id()
        .clone();
    let stale = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-transfer-stale-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTransfer {
                session_id: fixture.session_id.clone(),
                recipient_principal: bob_principal.as_str().to_owned(),
                expected_revision: 99,
                reason: None,
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&stale), "session_control_conflict");
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    let record = store.get(&fixture.session_id).await.unwrap().unwrap();
    assert_eq!(record.revision, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn transfer_to_ineligible_recipient_fails() {
    let fixture = lease_fixture("transfer-inelig").await;
    // Victor (Viewer) holds no `agent.invoke`: transfer never grants
    // capabilities the recipient does not already possess.
    let victor_principal = fixture
        .daemon
        .request_authority_for_client("client-victor")
        .principal_id()
        .clone();
    let denied = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-transfer-viewer-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTransfer {
                session_id: fixture.session_id.clone(),
                recipient_principal: victor_principal.as_str().to_owned(),
                expected_revision: 1,
                reason: None,
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert!(matches!(denied, CoreResponse::Error { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn controller_release_clears_lease() {
    let fixture = lease_fixture("release").await;
    let response = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-release-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlRelease {
                session_id: fixture.session_id.clone(),
                expected_revision: 1,
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert!(matches!(
        response,
        CoreResponse::SessionControlUpdated { controller: None }
    ));
    // Control fails closed after release until explicit takeover.
    assert_eq!(
        error_code(
            &steer_as(
                &fixture.daemon,
                "client-bob",
                &fixture.session_id,
                &fixture.turn_id
            )
            .await
        ),
        "session_control_not_controller"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn maintainer_takeover_succeeds_and_is_audited() {
    let fixture = lease_fixture("takeover").await;
    let response = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-takeover-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTakeover {
                session_id: fixture.session_id.clone(),
                expected_revision: 1,
                reason: "controller disconnected during incident".to_string(),
            },
        ),
        "client-cara",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::SessionControlUpdated { controller } => {
            let dto = controller.expect("takeover returns a lease");
            assert_eq!(dto.revision, 2);
        }
        other => panic!("expected takeover ack, got {other:?}"),
    }
    // Takeover is audited with structural locators only.
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT action, actor_principal, session_id FROM audit_event WHERE action = 'membership_change' AND session_id = ?",
    )
    .bind(&fixture.session_id)
    .fetch_all(fixture.team.pool())
    .await
    .unwrap();
    assert!(
        rows.iter()
            .any(|(action, _, _)| action == "membership_change"),
        "expected takeover audit, got {rows:?}"
    );
    // The takeover event carries ids/revision/action only.
    let events = fixture
        .daemon
        .event_log
        .replay_from(
            0,
            &codegg::core::event_log::EventFilter {
                session_id: Some(fixture.session_id.clone()),
                include_global: true,
                client_id: None,
            },
        )
        .await;
    assert!(
        events.iter().any(|envelope| matches!(
            &envelope.payload,
            codegg::protocol::core::CoreEvent::SessionControlChanged { action, .. }
            if action == "takeover"
        )),
        "expected SessionControlChanged(takeover) event"
    );
    // No secret material in the audit trail.
    let bodies: Vec<(String,)> =
        sqlx::query_as("SELECT metadata_json FROM audit_event WHERE session_id = ?")
            .bind(&fixture.session_id)
            .fetch_all(fixture.team.pool())
            .await
            .unwrap();
    for (metadata,) in bodies {
        assert!(!metadata.contains("cggt_"), "audit must not carry secrets");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn contributor_takeover_fails() {
    let fixture = lease_fixture("takeover-deny").await;
    // Contributors hold no `project.configure`: forced takeover is
    // denied at the gate with the privacy-safe shape (zero side
    // effect, lease untouched).
    let denied = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-takeover-bob-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTakeover {
                session_id: fixture.session_id.clone(),
                expected_revision: 1,
                reason: "force my way in".to_string(),
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "project_not_found");
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    let record = store.get(&fixture.session_id).await.unwrap().unwrap();
    assert_eq!(record.revision, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn takeover_without_lease_recovers_with_revision_zero() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    seed_session(team.pool(), &project, "sess-m004-recovery").await;
    for (name, role, client) in [
        ("Alice-rec", ProjectRole::Contributor, "client-alice"),
        ("Omar-rec", ProjectRole::Owner, "client-omar"),
    ] {
        let principal = human_token(&team, name, client).await;
        let record = team
            .get_principal(principal.principal_id())
            .await
            .unwrap()
            .unwrap();
        team.create_membership(&project, &record.id, role)
            .await
            .unwrap();
        register_client(&daemon, client, principal);
    }
    // Active turn with ambiguous provenance (no lease row, e.g.
    // pre-lease upgrade): install the runtime handle only.
    let runtime = daemon.sessions.get_or_create(
        "sess-m004-recovery",
        codegg_core::workspace::WorkspaceId::new_unchecked("ws-recovery"),
        std::path::PathBuf::from("/tmp"),
        project.as_str().to_owned(),
        std::path::PathBuf::from("/tmp"),
    );
    {
        let (cancel_tx, _rx) = tokio::sync::watch::channel(false);
        let mut active = runtime.active_turn.write().await;
        *active = Some(codegg::core::session_runtime::TurnHandle {
            turn_id: "turn-recovery".to_string(),
            cancel_tx,
            steer_tx: None,
            started_at: chrono::Utc::now(),
            asset_pin: None,
            controller_principal: None,
            controller_client: None,
            controller_revision: 0,
        });
    }
    // Ordinary control fails closed.
    assert_eq!(
        error_code(
            &cancel_as(
                &daemon,
                "client-alice",
                "sess-m004-recovery",
                "turn-recovery"
            )
            .await
        ),
        "session_control_not_controller"
    );
    // A nonzero revision for a fresh lease conflicts explicitly.
    let nonzero = Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-rec-nz-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTakeover {
                session_id: "sess-m004-recovery".to_string(),
                expected_revision: 7,
                reason: "recovery".to_string(),
            },
        ),
        "client-omar",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&nonzero), "session_control_conflict");
    // Revision zero establishes the recovery lease.
    let ok = Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-rec-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTakeover {
                session_id: "sess-m004-recovery".to_string(),
                expected_revision: 0,
                reason: "recovery after upgrade".to_string(),
            },
        ),
        "client-omar",
    ))
    .await
    .unwrap();
    assert!(
        matches!(ok, CoreResponse::SessionControlUpdated { .. }),
        "recovery takeover failed: {ok:?}"
    );
}

// ── Revocation ───────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn revoked_controller_loses_control() {
    let fixture = lease_fixture("revoke").await;
    // Revoke Alice's membership (Owner acts with the current revision).
    let membership = fixture
        .team
        .get_membership(
            &fixture.project,
            fixture
                .daemon
                .request_authority_for_client("client-alice")
                .principal_id(),
        )
        .await
        .unwrap()
        .unwrap();
    fixture
        .team
        .revoke_membership(
            &fixture.project,
            &membership.principal_id,
            membership.revision,
        )
        .await
        .unwrap();
    // The stale lease is immediately ineffective: a revoked
    // principal fails the capability gate itself (zero side effect),
    // so no stale controller authority survives revocation.
    let denied = steer_as(
        &fixture.daemon,
        "client-alice",
        &fixture.session_id,
        &fixture.turn_id,
    )
    .await;
    assert!(
        matches!(denied, CoreResponse::Error { .. }),
        "revoked controller must be denied, got {denied:?}"
    );
}

// ── Terminal release ─────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn terminal_transition_releases_lease() {
    let fixture = lease_fixture("terminal").await;
    fixture
        .daemon
        .release_turn_controller(&fixture.session_id, &fixture.turn_id)
        .await;
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    assert!(store.get(&fixture.session_id).await.unwrap().is_none());
    // A stale transfer racing completion changes nothing.
    let bob_principal = fixture
        .daemon
        .request_authority_for_client("client-bob")
        .principal_id()
        .clone();
    let raced = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-race-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlTransfer {
                session_id: fixture.session_id.clone(),
                recipient_principal: bob_principal.as_str().to_owned(),
                expected_revision: 1,
                reason: None,
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&raced), "session_control_not_found");
}

#[tokio::test(flavor = "current_thread")]
async fn reaper_releases_on_turn_completed_event() {
    let fixture = lease_fixture("reaper").await;
    fixture
        .daemon
        .spawn_turn_reaper(fixture.session_id.clone(), fixture.turn_id.clone());
    fixture
        .daemon
        .event_log
        .publish(
            Some(fixture.session_id.clone()),
            Some(fixture.turn_id.clone()),
            codegg::protocol::core::CoreEvent::TurnCompleted {
                session_id: fixture.session_id.clone(),
                turn_id: fixture.turn_id.clone(),
                stop_reason: "completed".to_string(),
            },
        )
        .await;
    // The reaper observes the terminal envelope and parks the turn.
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    let mut cleared = false;
    for _ in 0..100 {
        if store.get(&fixture.session_id).await.unwrap().is_none() {
            cleared = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(cleared, "reaper must release the lease on TurnCompleted");
}

// ── Submit acquisition + races ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::await_holding_lock)]
async fn turn_submit_atomically_acquires_controller() {
    let _env_guard = codegg::auth::test_support::lock_env();
    let previous = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "test-key-not-used");
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let alice = human_token(&team, "Alice-submit", "client-alice").await;
    let alice_record = team
        .get_principal(alice.principal_id())
        .await
        .unwrap()
        .unwrap();
    register_client(&daemon, "client-alice", alice);
    // Catalog project + workspace + session through the daemon so the
    // turn binds a real workspace.
    let workspace_id = match Box::pin(daemon.handle_request(new_request(
        "req-ws-submit".to_string(),
        CoreRequest::WorkspaceRegister {
            root: "/tmp".to_string(),
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match Box::pin(daemon.handle_request(new_request(
        "req-reg-submit".to_string(),
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "submit-probe".to_string(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_string(),
            },
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    let project = ProjectId::parse(&project_id).unwrap();
    team.create_membership(&project, &alice_record.id, ProjectRole::Contributor)
        .await
        .unwrap();
    let session_id = match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-create-submit".to_string(),
            CoreRequest::SessionCreate {
                directory: "/tmp".to_string(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap()
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("session create failed: {other:?}"),
    };
    let agent = codegg::protocol::dto::Agent {
        name: "test".to_string(),
        ..Default::default()
    };
    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-submit-ctrl".to_string(),
            CoreRequest::TurnSubmit {
                session_id: session_id.clone(),
                text: "hello".to_string(),
                plan_mode: false,
                model: "openai/gpt-4o".to_string(),
                agents: vec![agent],
                current_agent_idx: 0,
                messages: Vec::new(),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap();
    assert!(
        matches!(response, CoreResponse::Ack),
        "submit failed: {response:?}"
    );
    // The lease is established synchronously with acceptance.
    let store = SessionControllerStore::new(team.pool().clone());
    let record = store.get(&session_id).await.unwrap().expect("lease row");
    assert_eq!(record.controller_principal, alice_record.id);
    assert_eq!(record.revision, 1);
    // And the snapshot projects it (principal id plus coarse revision).
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-snap-submit".to_string(),
            CoreRequest::SnapshotSession {
                session_id: session_id.clone(),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap()
    {
        CoreResponse::SnapshotSession {
            controller_principal,
            controller_revision,
            ..
        } => {
            assert_eq!(
                controller_principal.as_deref(),
                Some(alice_record.id.as_str())
            );
            assert_eq!(controller_revision, Some(1));
        }
        other => panic!("expected snapshot, got {other:?}"),
    }
    if let Some(value) = previous {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

#[tokio::test(flavor = "current_thread")]
#[allow(clippy::await_holding_lock)]
async fn concurrent_submits_converge_on_one_controller() {
    let _env_guard = codegg::auth::test_support::lock_env();
    let previous = std::env::var("OPENAI_API_KEY").ok();
    std::env::set_var("OPENAI_API_KEY", "test-key-not-used");
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let workspace_id = match Box::pin(daemon.handle_request(new_request(
        "req-ws-race".to_string(),
        CoreRequest::WorkspaceRegister {
            root: "/tmp".to_string(),
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match Box::pin(daemon.handle_request(new_request(
        "req-reg-race".to_string(),
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "race-probe".to_string(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_string(),
            },
        },
    )))
    .await
    .unwrap()
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project register failed: {other:?}"),
    };
    let project = ProjectId::parse(&project_id).unwrap();
    for (name, client) in [("Alice-race", "client-alice"), ("Bob-race", "client-bob")] {
        let principal = human_token(&team, name, client).await;
        let record = team
            .get_principal(principal.principal_id())
            .await
            .unwrap()
            .unwrap();
        team.create_membership(&project, &record.id, ProjectRole::Contributor)
            .await
            .unwrap();
        register_client(&daemon, client, principal);
    }
    let session_id = match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-create-race".to_string(),
            CoreRequest::SessionCreate {
                directory: "/tmp".to_string(),
                title: None,
                project_id: Some(project_id),
                workspace_id: Some(workspace_id),
            },
        ),
        "client-alice",
    ))
    .await
    .unwrap()
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("session create failed: {other:?}"),
    };
    let agent = || codegg::protocol::dto::Agent {
        name: "test".to_string(),
        ..Default::default()
    };
    let submit_request = |tag: &str| {
        new_request(
            format!("req-race-{tag}"),
            CoreRequest::TurnSubmit {
                session_id: session_id.clone(),
                text: "hello".to_string(),
                plan_mode: false,
                model: "openai/gpt-4o".to_string(),
                agents: vec![agent()],
                current_agent_idx: 0,
                messages: Vec::new(),
            },
        )
    };
    let fut_alice = daemon.handle_request_for_client(submit_request("a"), "client-alice");
    let fut_bob = daemon.handle_request_for_client(submit_request("b"), "client-bob");
    let (alice_out, bob_out) = tokio::join!(fut_alice, fut_bob);
    let alice_out = alice_out.unwrap();
    let bob_out = bob_out.unwrap();
    // Exactly one submit wins; the loser observes turn contention and
    // leaves no second lease behind.
    let acks = [&alice_out, &bob_out]
        .iter()
        .filter(|response| matches!(response, CoreResponse::Ack))
        .count();
    assert_eq!(acks, 1, "one submit must win: {alice_out:?} vs {bob_out:?}");
    let store = SessionControllerStore::new(team.pool().clone());
    let record = store.get(&session_id).await.unwrap().expect("lease row");
    assert_eq!(record.revision, 1);
    if matches!(alice_out, CoreResponse::Ack) {
        let alice_id = daemon
            .request_authority_for_client("client-alice")
            .principal_id()
            .clone();
        assert_eq!(record.controller_principal, alice_id);
    } else {
        let bob_id = daemon
            .request_authority_for_client("client-bob")
            .principal_id()
            .clone();
        assert_eq!(record.controller_principal, bob_id);
    }
    if let Some(value) = previous {
        std::env::set_var("OPENAI_API_KEY", value);
    } else {
        std::env::remove_var("OPENAI_API_KEY");
    }
}

// ── Get / request protocol ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn control_get_and_request_round_trip() {
    let fixture = lease_fixture("get").await;
    let response = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-get-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlGet {
                session_id: fixture.session_id.clone(),
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    match response {
        CoreResponse::SessionControl { controller, .. } => {
            assert!(controller.is_some(), "lease must be visible");
        }
        other => panic!("expected control view, got {other:?}"),
    }
    // Bob requests control: inert (no lease change).
    let requested = Box::pin(fixture.daemon.handle_request_for_client(
        new_request(
            format!("req-req-{}", uuid::Uuid::new_v4()),
            CoreRequest::SessionControlRequest {
                session_id: fixture.session_id.clone(),
                message: Some("may I drive?".to_string()),
            },
        ),
        "client-bob",
    ))
    .await
    .unwrap();
    match requested {
        CoreResponse::SessionControl {
            controller,
            requests,
            ..
        } => {
            assert!(controller.is_some());
            assert_eq!(requests.len(), 1);
            assert_eq!(
                requests[0].requester_principal,
                fixture
                    .daemon
                    .request_authority_for_client("client-bob")
                    .principal_id()
                    .as_str()
            );
        }
        other => panic!("expected request ack, got {other:?}"),
    }
    let store = SessionControllerStore::new(fixture.team.pool().clone());
    let record = store.get(&fixture.session_id).await.unwrap().unwrap();
    assert_eq!(record.revision, 1, "requests must not mutate the lease");
}

// ── Restart reconciliation ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_derives_lease_from_trustworthy_attribution() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    seed_session(&pool, &project, "sess-m004-restart").await;
    let alice = human_token(&team, "Alice-restart", "client-alice").await;
    let alice_record = team
        .get_principal(alice.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &alice_record.id, ProjectRole::Contributor)
        .await
        .unwrap();
    // Active turn in the log with no lease row (pre-lease upgrade) plus
    // trustworthy origin attribution for the turn.
    sqlx::query(
        "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) VALUES (1, 'sess-m004-restart', 'turn-restart', 'turn_started', '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    let decision = codegg_core::authorization::AuthorizationDecision {
        principal_id: alice_record.id.clone(),
        operation: "turn_submit".to_string(),
        capability: Some("agent.invoke".to_string()),
        project_id: Some(project.clone()),
        membership_revision: None,
        policy: codegg_core::authorization::PolicyKind::TeamMembership,
        decision_id: "decision-restart".to_string(),
        correlation_id: "corr-restart".to_string(),
        reason: "test".to_string(),
        decided_at_ms: 1,
    };
    let attribution =
        codegg_core::authorization::OriginAttribution::from_authority(&alice, &decision);
    codegg_core::authorization::OriginAttributionStore::new(pool.clone())
        .record("turn", "turn-restart", &attribution)
        .await
        .unwrap();
    // recover_state emits TurnFailed for the interrupted turn... but
    // that would terminalize it. Instead reconcile directly against
    // the still-active turn to prove upgrade derivation.
    daemon.reconcile_session_controllers(&[]).await;
    let store = SessionControllerStore::new(pool);
    let record = store
        .get("sess-m004-restart")
        .await
        .unwrap()
        .expect("trustworthy attribution must derive a lease");
    assert_eq!(record.controller_principal, alice_record.id);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_with_legacy_attribution_fails_closed() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    seed_session(&pool, &project, "sess-m004-legacy").await;
    let omar = human_token(&team, "Omar-legacy", "client-omar").await;
    let omar_record = team
        .get_principal(omar.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &omar_record.id, ProjectRole::Owner)
        .await
        .unwrap();
    register_client(&daemon, "client-omar", omar);
    sqlx::query(
        "INSERT INTO core_event_log (event_seq, session_id, turn_id, event_type, payload_json) VALUES (1, 'sess-m004-legacy', 'turn-legacy', 'turn_started', '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    // Legacy provenance predates attribution: no lease is derived.
    codegg_core::authorization::OriginAttributionStore::new(pool.clone())
        .record(
            "turn",
            "turn-legacy",
            &codegg_core::authorization::OriginAttribution::legacy_local("corr-legacy"),
        )
        .await
        .unwrap();
    daemon.reconcile_session_controllers(&[]).await;
    let store = SessionControllerStore::new(pool);
    assert!(store.get("sess-m004-legacy").await.unwrap().is_none());
}
