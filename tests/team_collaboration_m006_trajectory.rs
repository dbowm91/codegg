//! Team Collaboration Corrective M006 — multi-user trajectory and security
//! qualification.
//!
//! Final cross-cutting gate over M001–M005. One reusable multi-principal
//! fixture (Alice Owner, Bob Contributor, Carol Viewer, Mallory outsider,
//! Alice second device; Project A/B) exercises the complete trajectory:
//!
//! - WP B: HTTP/Core authorization parity + event isolation.
//! - WP C: chat policy matrix (Viewer grant, Contributor deny, restricted
//!   channel, structured-action negatives, cross-project probes).
//! - WP D: team administration + token/membership revocation while clients
//!   stay connected.
//! - WP E: shared-session observe/chat/control handoff, permission/question
//!   gating, reconnect/restart convergence.
//! - WP F: Workspace dashboard filtering + rapid selected-project chat
//!   switching without cross-routing (daemon isolation plus TUI panel
//!   routing over the M005 surface).
//! - WP G: idempotent-retry/CAS contention convergence + secret-negative
//!   census.
//! - §9: pre-corrective compatibility (role-default chat) + LocalOwner
//!   solo mode without team setup.
//!
//! No new production semantics: harness-only qualification. Any defect found
//! here that violates an M001–M005 invariant would require a production fix
//! recorded in closure; none was found (see
//! `plans/closure/team-collaboration-corrective/006-status.md`).

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    ChatChannelModeDto, ChatMessageDto, ChatPolicyDecisionDto, CoreRequest, CoreResponse,
};
use codegg::server::authz::{
    authorize_enumeration, authorize_project, authorize_session, filter_visible_projects,
    require_local_owner, route_disposition_table, RouteDisposition,
};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::session_control::SessionControllerStore;
use codegg_core::team::{Capability, PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

// ── Fixture (WP A) ─────────────────────────────────────────────────────

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

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{client}-{}", uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

fn is_denied(response: &CoreResponse) -> bool {
    matches!(response, CoreResponse::Error { .. })
}

fn error_code(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code.clone(),
        other => panic!("expected error response, got {other:?}"),
    }
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

/// Multi-principal world: Alice owns A+B (two devices), Bob contributes to
/// A only, Carol views A only (project chat grant), Mallory is an outsider.
/// A has a default channel plus a restricted channel where Bob is denied;
/// B has a default channel. Carol's grant scopes to A only.
///
/// Projects are catalog-registered through the daemon (the M001 path) so
/// ProjectGet/WorkspaceDashboard resolve exactly like production.
struct World {
    daemon: CoreDaemon,
    team: TeamStore,
    project_a: ProjectId,
    project_b: ProjectId,
    session_a: String,
    session_b: String,
    alice: PrincipalId,
    bob: PrincipalId,
    carol: PrincipalId,
    mallory: PrincipalId,
    channel_a: String,
    channel_a_restricted: String,
    channel_b: String,
    _tmp_a: tempfile::TempDir,
    _tmp_b: tempfile::TempDir,
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
        other => panic!("workspace register failed, got {other:?}"),
    }
}

async fn register_project(daemon: &CoreDaemon, workspace_id: &str, name: &str) -> ProjectId {
    match daemon
        .handle_request(new_request(
            format!("req-reg-{name}"),
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
        CoreResponse::ProjectRegistered { project } => {
            ProjectId::parse(&project.project_id).expect("catalog project id parses")
        }
        other => panic!("project register failed, got {other:?}"),
    }
}

async fn world() -> World {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let tmp_a = tempfile::tempdir().expect("tempdir A");
    let tmp_b = tempfile::tempdir().expect("tempdir B");
    let workspace_a = register_workspace(&daemon, &tmp_a.path().display().to_string()).await;
    let workspace_b = register_workspace(&daemon, &tmp_b.path().display().to_string()).await;
    let project_a = register_project(&daemon, &workspace_a, "alpha").await;
    let project_b = register_project(&daemon, &workspace_b, "beta").await;
    let session_a = "sess-m006-a".to_owned();
    let session_b = "sess-m006-b".to_owned();
    seed_session(team.pool(), &project_a, &session_a).await;
    seed_session(team.pool(), &project_b, &session_b).await;

    // Alice: Owner of both projects, two devices.
    let alice_auth = human_token(&team, "m006-alice", "client-alice").await;
    let alice = alice_auth.principal_id().clone();
    team.create_membership(&project_a, &alice, ProjectRole::Owner)
        .await
        .unwrap();
    team.create_membership(&project_b, &alice, ProjectRole::Owner)
        .await
        .unwrap();
    let alice2 = second_device(&team, &alice, "client-alice-2").await;

    // Bob: Contributor in A only.
    let bob_auth = human_token(&team, "m006-bob", "client-bob").await;
    let bob = bob_auth.principal_id().clone();
    team.create_membership(&project_a, &bob, ProjectRole::Contributor)
        .await
        .unwrap();

    // Carol: Viewer in A only.
    let carol_auth = human_token(&team, "m006-carol", "client-carol").await;
    let carol = carol_auth.principal_id().clone();
    team.create_membership(&project_a, &carol, ProjectRole::Viewer)
        .await
        .unwrap();

    // Mallory: authenticated principal with no membership anywhere.
    let mallory_auth = human_token(&team, "m006-mallory", "client-mallory").await;
    let mallory = mallory_auth.principal_id().clone();

    for (client, auth) in [
        ("client-alice", alice_auth),
        ("client-alice-2", alice2),
        ("client-bob", bob_auth),
        ("client-carol", carol_auth),
        ("client-mallory", mallory_auth),
    ] {
        register_client(&daemon, client, auth);
    }

    // Channels: Alice ensures all three under Owner authority.
    let channel_a = ensure_channel(&daemon, "client-alice", project_a.as_str(), None).await;
    let channel_a_restricted = ensure_channel(
        &daemon,
        "client-alice",
        project_a.as_str(),
        Some("restricted"),
    )
    .await;
    let channel_b = ensure_channel(&daemon, "client-alice", project_b.as_str(), None).await;

    // Restricted channel: locked mode + explicit Bob denial.
    match call(
        &daemon,
        "client-alice",
        CoreRequest::ChatChannelPolicySet {
            channel_id: channel_a_restricted.clone(),
            mode: Some(ChatChannelModeDto::Restricted),
            principal_id: None,
            decision: None,
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { .. } => {}
        other => panic!("expected channel mode set, got {other:?}"),
    }
    match call(
        &daemon,
        "client-alice",
        CoreRequest::ChatChannelPolicySet {
            channel_id: channel_a_restricted.clone(),
            mode: None,
            principal_id: Some(bob.as_str().to_owned()),
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { .. } => {}
        other => panic!("expected bob channel deny, got {other:?}"),
    }
    // Carol: project chat grant in A only.
    match call(
        &daemon,
        "client-alice",
        CoreRequest::ChatProjectPolicySet {
            project_id: project_a.as_str().to_owned(),
            principal_id: carol.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { .. } => {}
        other => panic!("expected carol project grant, got {other:?}"),
    }

    World {
        daemon,
        team,
        project_a,
        project_b,
        session_a,
        session_b,
        alice,
        bob,
        carol,
        mallory,
        channel_a,
        channel_a_restricted,
        channel_b,
        _tmp_a: tmp_a,
        _tmp_b: tmp_b,
    }
}

async fn ensure_channel(
    daemon: &CoreDaemon,
    client: &str,
    project: &str,
    name: Option<&str>,
) -> String {
    match call(
        daemon,
        client,
        CoreRequest::ChatChannelEnsure {
            project_id: project.to_owned(),
            name: name.map(str::to_owned),
        },
    )
    .await
    {
        CoreResponse::ChatChannel { channel } => {
            assert_eq!(channel.project_id, project);
            channel.channel_id
        }
        other => panic!("expected chat channel, got {other:?}"),
    }
}

async fn send(
    daemon: &CoreDaemon,
    client: &str,
    channel: &str,
    body: &str,
    key: Option<String>,
) -> CoreResponse {
    call(
        daemon,
        client,
        CoreRequest::ChatSend {
            channel_id: channel.to_owned(),
            body: body.to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: key,
        },
    )
    .await
}

async fn send_ok(daemon: &CoreDaemon, client: &str, channel: &str, body: &str) {
    match send(
        daemon,
        client,
        channel,
        body,
        Some(uuid::Uuid::new_v4().to_string()),
    )
    .await
    {
        CoreResponse::ChatMessage { .. } => {}
        other => panic!("expected chat message, got {other:?}"),
    }
}

async fn history_len(daemon: &CoreDaemon, client: &str, channel: &str) -> usize {
    match call(
        daemon,
        client,
        CoreRequest::ChatHistory {
            channel_id: channel.to_owned(),
            from_seq: None,
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => messages.len(),
        other => panic!("expected chat history, got {other:?}"),
    }
}

/// Install one active turn in `session` with a durable lease held by Alice
/// (M004 pattern: in-memory handle + durable acquire). Returns the turn id
/// and keeps the runtime channels alive via the returned guards.
async fn install_alice_turn(
    world: &World,
    session: &str,
    tag: &str,
) -> (
    String,
    tokio::sync::watch::Receiver<bool>,
    tokio::sync::mpsc::Receiver<String>,
) {
    let project = if session == world.session_a.as_str() {
        world.project_a.as_str()
    } else {
        world.project_b.as_str()
    };
    let runtime = world.daemon.sessions.get_or_create(
        session,
        codegg_core::workspace::WorkspaceId::new_unchecked(format!("ws-m006-{tag}")),
        std::path::PathBuf::from("/tmp"),
        project.to_owned(),
        std::path::PathBuf::from("/tmp"),
    );
    let turn_id = format!("turn-m006-{tag}");
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
            controller_principal: Some(world.alice.as_str().to_owned()),
            controller_client: Some("client-alice".to_string()),
            controller_revision: 1,
        });
    }
    SessionControllerStore::new(world.team.pool().clone())
        .acquire(session, &turn_id, &world.alice, Some("client-alice"), 1)
        .await
        .unwrap();
    (turn_id, cancel_rx, steer_rx)
}

// ── WP B: HTTP/Core parity + event isolation ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn http_core_parity_for_reads_and_enumeration() {
    let w = world().await;
    let pool = w.team.pool();

    // Bob: member of A only. Core and HTTP adapter must agree.
    let bob = w.daemon.clients.principal_for("client-bob").unwrap();
    assert!(
        !is_denied(
            &call(
                &w.daemon,
                "client-bob",
                CoreRequest::ProjectGet {
                    project_id: w.project_a.as_str().to_owned()
                },
            )
            .await
        ),
        "bob reads project A via Core"
    );
    assert!(
        authorize_project(
            pool,
            &bob,
            &w.project_a,
            Capability::ProjectRead,
            "m006_parity"
        )
        .await
        .is_ok(),
        "bob reads project A via HTTP adapter"
    );
    // Bob in B: both surfaces deny with the privacy-safe shape.
    let core_b = call(
        &w.daemon,
        "client-bob",
        CoreRequest::ProjectGet {
            project_id: w.project_b.as_str().to_owned(),
        },
    )
    .await;
    assert!(is_denied(&core_b));
    assert_eq!(error_code(&core_b), "project_not_found");
    assert!(
        authorize_project(
            pool,
            &bob,
            &w.project_b,
            Capability::ProjectRead,
            "m006_parity"
        )
        .await
        .is_err(),
        "HTTP adapter denies bob in B"
    );
    // Session resolution is server-side: Bob's B session probe fails closed
    // on both surfaces. The Core gate uses its accepted typed code while
    // the HTTP adapter maps the same denial to the privacy-safe shape
    // (M001 §5: typed Core codes are by design; the network boundary is
    // where existence becomes indistinguishable).
    let core_sess = call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionLoad {
            session_id: w.session_b.clone(),
        },
    )
    .await;
    assert!(is_denied(&core_sess));
    assert!(
        authorize_session(
            pool,
            &bob,
            &w.session_b,
            Capability::SessionRead,
            "m006_parity"
        )
        .await
        .is_err(),
        "HTTP adapter denies bob session B"
    );
    // Mallory: denied everywhere on both surfaces, indistinguishable from
    // absence.
    let mallory = w.daemon.clients.principal_for("client-mallory").unwrap();
    for project in [&w.project_a, &w.project_b] {
        let resp = call(
            &w.daemon,
            "client-mallory",
            CoreRequest::ProjectGet {
                project_id: project.as_str().to_owned(),
            },
        )
        .await;
        assert!(is_denied(&resp));
        assert_eq!(error_code(&resp), "project_not_found");
        assert!(authorize_project(
            pool,
            &mallory,
            project,
            Capability::ProjectRead,
            "m006_parity"
        )
        .await
        .is_err());
    }
    // Enumeration parity: Bob sees exactly A; Mallory sees nothing.
    assert!(
        authorize_enumeration(pool, &bob, "m006_parity")
            .await
            .is_ok(),
        "active bob may enumerate"
    );
    let candidates = vec![w.project_a.clone(), w.project_b.clone()];
    let visible = filter_visible_projects(pool, &bob, &candidates).await;
    assert_eq!(visible, vec![w.project_a.clone()]);
    let mallory_visible = filter_visible_projects(pool, &mallory, &candidates).await;
    assert!(
        mallory_visible.is_empty(),
        "mallory learns no project identities"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn global_event_surface_stays_local_owner_only() {
    let w = world().await;
    // Static disposition: the global event stream is LocalOwner-only.
    let entry = route_disposition_table()
        .into_iter()
        .find(|row| row.method == "GET" && row.path == "/api/event")
        .expect("event route has a disposition");
    assert_eq!(entry.disposition, RouteDisposition::LocalOwnerOnly);

    // Team principals fail the LocalOwner gate; the LocalOwner passes.
    let pool = w.team.pool();
    let bob = w.daemon.clients.principal_for("client-bob").unwrap();
    assert!(
        require_local_owner(pool, &bob, "m006_event_probe")
            .await
            .is_err(),
        "bob must not hold the global event stream"
    );
    let local = AuthenticatedPrincipal::local_owner("client-local");
    assert!(
        require_local_owner(pool, &local, "m006_event_probe")
            .await
            .is_ok(),
        "local owner keeps compatibility stream"
    );
}

// ── WP C: chat policy matrix ───────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn chat_policy_matrix_end_to_end() {
    let w = world().await;

    // Carol (Viewer + A grant) chats in the allowed scope …
    send_ok(&w.daemon, "client-carol", &w.channel_a, "carol question A").await;
    // … but her grant never crosses into B.
    let carol_b = send(
        &w.daemon,
        "client-carol",
        &w.channel_b,
        "carol probe B",
        None,
    )
    .await;
    assert!(is_denied(&carol_b));
    assert_eq!(error_code(&carol_b), "project_not_found");
    let carol_b_hist = call(
        &w.daemon,
        "client-carol",
        CoreRequest::ChatHistory {
            channel_id: w.channel_b.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(is_denied(&carol_b_hist));

    // Bob (Contributor) keeps the compatible default in the general
    // channel …
    send_ok(&w.daemon, "client-bob", &w.channel_a, "bob hello A").await;
    // … but is denied the restricted channel while Carol's explicit grant
    // still reaches it (channel grant path differs from project default).
    let bob_restricted = send(
        &w.daemon,
        "client-bob",
        &w.channel_a_restricted,
        "bob restricted probe",
        None,
    )
    .await;
    assert!(is_denied(&bob_restricted));
    let code = error_code(&bob_restricted);
    assert!(
        code == "project_not_found" || code == "chat_channel_not_found",
        "restricted denial is privacy-safe, got {code}"
    );
    // Denied-channel history is indistinguishable from absence.
    let bob_hist = call(
        &w.daemon,
        "client-bob",
        CoreRequest::ChatHistory {
            channel_id: w.channel_a_restricted.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(is_denied(&bob_hist));

    // Mallory learns nothing: every entry point denies privacy-safe.
    for request in [
        CoreRequest::ChatChannelList {
            project_id: w.project_a.as_str().to_owned(),
            limit: None,
        },
        CoreRequest::ChatHistory {
            channel_id: w.channel_a.clone(),
            from_seq: None,
            limit: None,
        },
        CoreRequest::ChatSync {
            channel_id: w.channel_a.clone(),
            from_seq: 0,
            limit: None,
        },
    ] {
        let resp = call(&w.daemon, "client-mallory", request).await;
        assert!(is_denied(&resp), "mallory entry denied");
        assert_eq!(error_code(&resp), "project_not_found");
    }

    // Chat never grants execution: Carol cannot submit turns and her
    // structured action (no semantic capability) creates nothing.
    let turn = call(
        &w.daemon,
        "client-carol",
        CoreRequest::TurnSubmit {
            session_id: w.session_a.clone(),
            text: "escalate".to_owned(),
            plan_mode: false,
            model: "m".to_owned(),
            agents: Vec::new(),
            current_agent_idx: 0,
            messages: Vec::new(),
        },
    )
    .await;
    assert!(is_denied(&turn), "viewer chat grant is not execution");
    let before = history_len(&w.daemon, "client-alice", &w.channel_a).await;
    let action = call(
        &w.daemon,
        "client-carol",
        CoreRequest::ChatActionSubmit {
            channel_id: w.channel_a.clone(),
            message_id: "msg-m006-nope".to_owned(),
            action: codegg::protocol::core::ChatActionSubmitDto::JobReference {
                job_id: "job-m006-nope".to_owned(),
                title: None,
            },
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        },
    )
    .await;
    assert!(is_denied(&action), "structured action needs semantic cap");
    assert_eq!(
        history_len(&w.daemon, "client-alice", &w.channel_a).await,
        before,
        "denied action leaves no side effect"
    );

    // Cross-project locator probe: Bob's A membership says nothing about B.
    let probe = call(
        &w.daemon,
        "client-bob",
        CoreRequest::ChatHistory {
            channel_id: w.channel_b.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(is_denied(&probe));
    assert_eq!(error_code(&probe), "project_not_found");
}

// ── WP D: administration + revocation while connected ──────────────────

async fn membership_revision(
    daemon: &CoreDaemon,
    client: &str,
    project: &str,
    principal: &PrincipalId,
) -> u64 {
    match call(
        daemon,
        client,
        CoreRequest::TeamMembershipList {
            project_id: project.to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembershipList { memberships, .. } => {
            memberships
                .iter()
                .find(|m| m.principal_id == principal.as_str())
                .unwrap_or_else(|| panic!("membership row missing"))
                .revision
        }
        other => panic!("expected membership list, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn team_administration_and_revocation_while_connected() {
    let w = world().await;

    // Bob is live before revocation.
    send_ok(&w.daemon, "client-bob", &w.channel_a, "bob before revoke").await;

    // Non-owners cannot administer: Bob (Contributor) and Carol (Viewer)
    // fail the member.manage gate in their own project.
    for client in ["client-bob", "client-carol"] {
        let resp = call(
            &w.daemon,
            client,
            CoreRequest::TeamMembershipList {
                project_id: w.project_a.as_str().to_owned(),
                limit: None,
            },
        )
        .await;
        assert!(is_denied(&resp), "{client} must not list membership");
    }
    // Alice owns A but her authority stops at B's boundary: she cannot
    // administer a project through a foreign locator, and Bob cannot
    // self-administer.
    let foreign = call(
        &w.daemon,
        "client-bob",
        CoreRequest::TeamMembershipAdd {
            project_id: w.project_b.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            role: "contributor".to_owned(),
        },
    )
    .await;
    assert!(is_denied(&foreign));

    // Stale revisions conflict without overwrite.
    let rev = membership_revision(&w.daemon, "client-alice", w.project_a.as_str(), &w.bob).await;
    let stale = call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipUpdate {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            expected_revision: rev + 99,
            role: Some("maintainer".to_owned()),
            state: None,
        },
    )
    .await;
    assert!(is_denied(&stale));
    assert_eq!(error_code(&stale), "team_revision_conflict");

    // Alice revokes Bob while his client stays connected: the next
    // request boundary denies chat, reads, and control.
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipRevoke {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            expected_revision: rev,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { .. } => {}
        other => panic!("expected revoke, got {other:?}"),
    }
    let after_chat = send(
        &w.daemon,
        "client-bob",
        &w.channel_a,
        "bob after revoke",
        None,
    )
    .await;
    assert!(is_denied(&after_chat));
    let after_read = call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionLoad {
            session_id: w.session_a.clone(),
        },
    )
    .await;
    assert!(is_denied(&after_read));
    let after_control = call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionControlGet {
            session_id: w.session_a.clone(),
        },
    )
    .await;
    assert!(is_denied(&after_control));
    // Revocation is monotonic: the row survives as revoked.
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipList {
            project_id: w.project_a.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembershipList { memberships, .. } => {
            let row = memberships
                .iter()
                .find(|m| m.principal_id == w.bob.as_str())
                .expect("revoked row retained");
            assert_ne!(row.state, "active");
        }
        other => panic!("expected membership list, got {other:?}"),
    }
}

// ── WP E: observe/chat/control handoff + reconnect/restart ─────────────

#[tokio::test(flavor = "current_thread")]
async fn controller_transfer_handoff_between_alice_and_bob() {
    let w = world().await;
    let (turn_id, _cancel, _steer) = install_alice_turn(&w, &w.session_a, "handoff").await;

    // Non-controller mutation fails: Bob observes/chats but cannot steer.
    let denied = call(
        &w.daemon,
        "client-bob",
        CoreRequest::TurnSteer {
            session_id: w.session_a.clone(),
            turn_id: turn_id.clone(),
            text: "bob redirect".to_owned(),
        },
    )
    .await;
    assert!(is_denied(&denied));
    assert_eq!(error_code(&denied), "session_control_not_controller");

    // Alice's second device shares her principal: same-principal resume.
    let allowed = call(
        &w.daemon,
        "client-alice-2",
        CoreRequest::TurnSteer {
            session_id: w.session_a.clone(),
            turn_id: turn_id.clone(),
            text: "alice second device".to_owned(),
        },
    )
    .await;
    assert!(!is_denied(&allowed), "same principal second device steers");

    // Explicit handoff: Alice transfers to Bob under CAS revision.
    let revision = match call(
        &w.daemon,
        "client-alice",
        CoreRequest::SessionControlGet {
            session_id: w.session_a.clone(),
        },
    )
    .await
    {
        CoreResponse::SessionControl { controller, .. } => controller.expect("lease held").revision,
        other => panic!("expected control get, got {other:?}"),
    };
    // Stale revision fails without overwrite.
    let stale = call(
        &w.daemon,
        "client-alice",
        CoreRequest::SessionControlTransfer {
            session_id: w.session_a.clone(),
            recipient_principal: w.bob.as_str().to_owned(),
            expected_revision: revision + 7,
            reason: None,
        },
    )
    .await;
    assert!(is_denied(&stale));
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::SessionControlTransfer {
            session_id: w.session_a.clone(),
            recipient_principal: w.bob.as_str().to_owned(),
            expected_revision: revision,
            reason: Some("handoff to bob".to_owned()),
        },
    )
    .await
    {
        CoreResponse::SessionControlUpdated { controller } => {
            assert_eq!(
                controller.expect("new lease").controller_principal,
                w.bob.as_str()
            );
        }
        other => panic!("expected transfer, got {other:?}"),
    }
    // Control moved: Bob steers, Alice is now the denied observer.
    assert!(!is_denied(
        &call(
            &w.daemon,
            "client-bob",
            CoreRequest::TurnSteer {
                session_id: w.session_a.clone(),
                turn_id: turn_id.clone(),
                text: "bob now controls".to_owned(),
            },
        )
        .await
    ));
    let alice_after = call(
        &w.daemon,
        "client-alice",
        CoreRequest::TurnSteer {
            session_id: w.session_a.clone(),
            turn_id: turn_id.clone(),
            text: "alice after handoff".to_owned(),
        },
    )
    .await;
    assert!(is_denied(&alice_after));
    assert_eq!(error_code(&alice_after), "session_control_not_controller");

    // Transfer to an ineligible recipient (Viewer Carol) fails closed.
    let bad_recipient = call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionControlTransfer {
            session_id: w.session_a.clone(),
            recipient_principal: w.carol.as_str().to_owned(),
            expected_revision: revision + 1,
            reason: None,
        },
    )
    .await;
    assert!(is_denied(&bad_recipient));
}

#[tokio::test(flavor = "current_thread")]
async fn permission_question_and_control_follow_revocation_and_reconnect() {
    let w = world().await;
    let (_turn, _cancel, _steer) = install_alice_turn(&w, &w.session_a, "permrev").await;

    // Unknown pending ids fail closed for members and outsiders alike:
    // no existence oracle, no mutation.
    for client in ["client-bob", "client-mallory"] {
        let perm = call(
            &w.daemon,
            client,
            CoreRequest::PermissionRespond {
                id: "perm-m006-absent".to_owned(),
                choice: "allow".to_owned(),
            },
        )
        .await;
        assert!(is_denied(&perm), "{client} pending respond fails closed");
        let question = call(
            &w.daemon,
            client,
            CoreRequest::QuestionRespond {
                id: "q-m006-absent".to_owned(),
                answers: serde_json::json!({}),
            },
        )
        .await;
        assert!(is_denied(&question), "{client} question fails closed");
    }

    // Revoking the controller denies control at the next boundary, and a
    // reconnect (fresh client registration, same revoked principal) stays
    // denied: revocation is re-evaluated per request, never cached.
    let rev = membership_revision(&w.daemon, "client-alice", w.project_a.as_str(), &w.bob).await;
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipRevoke {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            expected_revision: rev,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { .. } => {}
        other => panic!("expected revoke, got {other:?}"),
    }
    // Simulate reconnect: drop and re-register Bob's client binding.
    w.daemon.clients.unregister("client-bob");
    let tokens = PersonalTokenStore::with_team(w.team.pool().clone(), w.team.clone());
    let (plaintext, _) = tokens
        .create_personal_token(&w.bob, "device-reconnect", None)
        .await
        .unwrap();
    // Token issuance still works (principal exists) but every project
    // request denies: membership is gone.
    assert!(tokens
        .verify_for_client(&plaintext, "client-bob")
        .await
        .is_ok());
    let rebased = tokens
        .verify_for_client(&plaintext, "client-bob")
        .await
        .unwrap();
    register_client(&w.daemon, "client-bob", rebased);
    let after = call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionControlRequest {
            session_id: w.session_a.clone(),
            message: None,
        },
    )
    .await;
    assert!(
        is_denied(&after),
        "revoked principal denied after reconnect"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn restart_preserves_policy_and_revocation() {
    let w = world().await;
    send_ok(
        &w.daemon,
        "client-carol",
        &w.channel_a,
        "carol before restart",
    )
    .await;
    let rev = membership_revision(&w.daemon, "client-alice", w.project_a.as_str(), &w.bob).await;
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipRevoke {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            expected_revision: rev,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { .. } => {}
        other => panic!("expected revoke, got {other:?}"),
    }

    // Restart: a fresh daemon over the same durable pool.
    let pool = w.team.pool().clone();
    let daemon2 = CoreDaemon::new(Some(pool.clone()), None, None);
    let team2 = TeamStore::new(pool);
    async fn reauth(
        team: &TeamStore,
        principal: &PrincipalId,
        client: &str,
    ) -> AuthenticatedPrincipal {
        let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
        let (plaintext, _) = tokens
            .create_personal_token(principal, "device-restart", None)
            .await
            .unwrap();
        tokens.verify_for_client(&plaintext, client).await.unwrap()
    }
    let alice2 = reauth(&team2, &w.alice, "client-alice").await;
    let bob2 = reauth(&team2, &w.bob, "client-bob").await;
    let carol2 = reauth(&team2, &w.carol, "client-carol").await;
    register_client(&daemon2, "client-alice", alice2);
    register_client(&daemon2, "client-bob", bob2);
    register_client(&daemon2, "client-carol", carol2);

    // Policy survives restart: Carol's A grant still holds, Bob's
    // restricted denial still holds, revoked Bob still fails everywhere.
    send_ok(
        &daemon2,
        "client-carol",
        &w.channel_a,
        "carol after restart",
    )
    .await;
    let bob_denied = send(
        &daemon2,
        "client-bob",
        &w.channel_a,
        "bob after restart",
        None,
    )
    .await;
    assert!(is_denied(&bob_denied));
    let bob_restricted = send(
        &daemon2,
        "client-bob",
        &w.channel_a_restricted,
        "bob restricted after restart",
        None,
    )
    .await;
    assert!(is_denied(&bob_restricted));
    // Pre-restart history is durable: Carol's first message persists.
    assert!(history_len(&daemon2, "client-carol", &w.channel_a).await >= 2);
}

// ── WP F: Workspace routing ────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn workspace_dashboard_filters_by_membership_and_revocation() {
    let w = world().await;

    // Bob sees exactly A; Mallory sees nothing; Alice sees both.
    for (client, expected) in [
        (
            "client-alice",
            vec![w.project_a.as_str(), w.project_b.as_str()],
        ),
        ("client-bob", vec![w.project_a.as_str()]),
        ("client-mallory", vec![]),
    ] {
        match call(
            &w.daemon,
            client,
            CoreRequest::WorkspaceDashboard {
                cursor: None,
                limit: None,
                include_archived: false,
            },
        )
        .await
        {
            CoreResponse::WorkspaceDashboard { rows, .. } => {
                let mut ids: Vec<&str> = rows.iter().map(|r| r.project_id.as_str()).collect();
                ids.sort_unstable();
                let mut want = expected.clone();
                want.sort_unstable();
                assert_eq!(ids, want, "{client} dashboard rows");
            }
            other => panic!("expected dashboard, got {other:?}"),
        }
    }

    // Revocation removes the row at the next boundary (no cross-fallback).
    let rev = membership_revision(&w.daemon, "client-alice", w.project_a.as_str(), &w.bob).await;
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipRevoke {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.bob.as_str().to_owned(),
            expected_revision: rev,
        },
    )
    .await
    {
        CoreResponse::TeamMembership { .. } => {}
        other => panic!("expected revoke, got {other:?}"),
    }
    match call(
        &w.daemon,
        "client-bob",
        CoreRequest::WorkspaceDashboard {
            cursor: None,
            limit: None,
            include_archived: false,
        },
    )
    .await
    {
        CoreResponse::WorkspaceDashboard { rows, .. } => {
            assert!(rows.is_empty(), "revoked bob sees no rows");
        }
        other => panic!("expected dashboard, got {other:?}"),
    }

    // Chat isolation across projects: A traffic never surfaces in B.
    send_ok(&w.daemon, "client-alice", &w.channel_a, "only in A").await;
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatSync {
            channel_id: w.channel_b.clone(),
            from_seq: 0,
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatSync { messages, .. } => {
            assert!(
                messages.iter().all(|m| m.body != "only in A"),
                "no cross-project chat leak"
            );
        }
        other => panic!("expected sync, got {other:?}"),
    }
}

// TUI panel routing across rapid selection changes (M005 surface).

use async_trait::async_trait;
use codegg::core::CoreClient;
use codegg::error::AppError;
use codegg::protocol::core::{ChatChannelDto, CoreEvent, EventEnvelope, RequestEnvelope};
use codegg::protocol::work_order::ProjectActivitySummaryDto;
use codegg::tui::app::state::{ProjectTabState, ProjectTabs, WorkspaceDashboardState};
use codegg::tui::app::TuiMsg;
use codegg::tui::route::Route;

struct FakeM006Client;

impl FakeM006Client {
    fn row(project_id: &str, display_name: &str) -> ProjectActivitySummaryDto {
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
impl CoreClient for FakeM006Client {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        Ok(match request.payload {
            CoreRequest::WorkspaceDashboard { .. } => CoreResponse::WorkspaceDashboard {
                rows: vec![
                    Self::row("project-a", "Alpha"),
                    Self::row("project-b", "Beta"),
                ],
                next_cursor: None,
                truncated: false,
            },
            CoreRequest::ChatChannelEnsure { project_id, .. } => CoreResponse::ChatChannel {
                channel: ChatChannelDto {
                    channel_id: format!("ch-{project_id}"),
                    project_id: project_id.clone(),
                    name: "general".to_string(),
                    created_by: "alice".to_string(),
                    created_at_ms: 1,
                },
            },
            CoreRequest::ChatHistory { channel_id, .. } => CoreResponse::ChatHistory {
                channel_id,
                messages: Vec::new(),
                next_cursor: 0,
                truncated: false,
                retention_floor_seq: 0,
            },
            CoreRequest::ChatSync { channel_id, .. } => CoreResponse::ChatSync {
                channel_id,
                messages: Vec::new(),
                next_cursor: 0,
                resync_required: false,
                retention_floor_seq: 0,
            },
            _ => CoreResponse::Error {
                code: "unsupported_in_test".to_string(),
                message: "fake m006 client".to_string(),
            },
        })
    }

    fn subscribe(&self) -> tokio::sync::mpsc::Receiver<EventEnvelope<CoreEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        rx
    }
}

fn m006_message(project_id: &str, seq: u64, body: &str) -> ChatMessageDto {
    ChatMessageDto {
        message_id: format!("msg-{project_id}-{seq}"),
        channel_id: format!("ch-{project_id}"),
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

fn seed_ready(app: &mut codegg::tui::app::App, project_id: &str, bodies: &[&str]) {
    let channel_id = format!("ch-{project_id}");
    let Some(request_id) = app.chat.begin_history(project_id, &channel_id) else {
        panic!("begin_history for {project_id}");
    };
    let epoch = app.chat.reconnect_epoch;
    let messages: Vec<ChatMessageDto> = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| m006_message(project_id, (i + 1) as u64, body))
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

#[test]
fn workspace_rapid_switch_never_cross_routes_chat_or_prompt() {
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
    app.set_core_client(Arc::new(FakeM006Client));
    let (tx, _rx) = tokio::sync::mpsc::channel(32);
    app.tui_cmd_tx = Some(tx);

    // Open the Workspace view with both projects and seed per-project chat.
    let return_tab = app.project_tabs.active_tab_id().cloned();
    let epoch = app.routing_registry.reconnect_epoch;
    let mut dashboard = WorkspaceDashboardState::new(return_tab, epoch);
    let gen = dashboard.begin_refresh();
    dashboard.apply_loaded(
        gen,
        vec![
            FakeM006Client::row("project-a", "Alpha"),
            FakeM006Client::row("project-b", "Beta"),
        ],
        false,
        None,
    );
    app.dialog_state.workspace_dashboard = Some(dashboard);
    app.ui_state.routes.navigate_to(Route::Workspace);
    seed_ready(&mut app, "project-a", &["alpha-only"]);
    seed_ready(&mut app, "project-b", &["beta-only"]);

    // Rapid selection flapping: A -> B -> A -> B. The panel must always
    // show the selected project's chat and never the other's.
    let now_ms = 1_000_000;
    for (delta, selected, present, absent) in [
        (1, "project-b", "beta-only", "alpha-only"),
        (-1, "project-a", "alpha-only", "beta-only"),
        (1, "project-b", "beta-only", "alpha-only"),
        (-1, "project-a", "alpha-only", "beta-only"),
    ] {
        app.process_msg(TuiMsg::WorkspaceDashboardMove { delta });
        let selected_id = app
            .dialog_state
            .workspace_dashboard
            .as_ref()
            .and_then(|d| d.selected_project_id());
        assert_eq!(selected_id.as_deref(), Some(selected));
        let lines = app.chat.panel_lines(selected, now_ms);
        assert!(
            lines.iter().any(|l| l.contains(present)),
            "panel shows {selected} chat"
        );
        assert!(
            !lines.iter().any(|l| l.contains(absent)),
            "panel never cross-routes {absent} into {selected}"
        );
    }

    // Per-project drafts survive the flapping without crossing.
    app.chat.set_draft("project-a", "draft A".to_string());
    app.chat.set_draft("project-b", "draft B".to_string());
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: 1 });
    assert_eq!(app.chat.draft_for("project-b"), "draft B");
    app.process_msg(TuiMsg::WorkspaceDashboardMove { delta: -1 });
    assert_eq!(app.chat.draft_for("project-a"), "draft A");
}

// ── WP G: contention convergence + secret-negative census ─────────────

#[tokio::test(flavor = "current_thread")]
async fn idempotent_retries_and_cas_contention_converge() {
    let w = world().await;

    // Duplicate chat send with the same idempotency key converges: the
    // second call returns the original message (duplicate flag) with no
    // second sequence number.
    let key = uuid::Uuid::new_v4().to_string();
    let first = send(
        &w.daemon,
        "client-alice",
        &w.channel_a,
        "retry me",
        Some(key.clone()),
    )
    .await;
    let (first_id, first_seq) = match &first {
        CoreResponse::ChatMessage { message, .. } => (message.message_id.clone(), message.seq),
        other => panic!("expected message, got {other:?}"),
    };
    let second = send(
        &w.daemon,
        "client-alice",
        &w.channel_a,
        "retry me",
        Some(key.clone()),
    )
    .await;
    match &second {
        CoreResponse::ChatMessage { message, duplicate } => {
            assert_eq!(message.message_id, first_id);
            assert_eq!(message.seq, first_seq);
            assert!(*duplicate, "retry converges with duplicate flag");
        }
        other => panic!("expected converged message, got {other:?}"),
    }
    assert_eq!(
        history_len(&w.daemon, "client-alice", &w.channel_a).await,
        1
    );

    // Duplicate control requests are inert: no lease mutation, one bounded
    // row set, revision untouched.
    let (_turn, _cancel, _steer) = install_alice_turn(&w, &w.session_a, "idem").await;
    let rev_before = match call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionControlGet {
            session_id: w.session_a.clone(),
        },
    )
    .await
    {
        CoreResponse::SessionControl { controller, .. } => controller.expect("lease").revision,
        other => panic!("expected control get, got {other:?}"),
    };
    for _ in 0..3 {
        match call(
            &w.daemon,
            "client-bob",
            CoreRequest::SessionControlRequest {
                session_id: w.session_a.clone(),
                message: Some("please".to_owned()),
            },
        )
        .await
        {
            CoreResponse::SessionControl { .. } => {}
            other => panic!("expected request ack, got {other:?}"),
        }
    }
    let rev_after = match call(
        &w.daemon,
        "client-bob",
        CoreRequest::SessionControlGet {
            session_id: w.session_a.clone(),
        },
    )
    .await
    {
        CoreResponse::SessionControl { controller, .. } => controller.expect("lease").revision,
        other => panic!("expected control get, got {other:?}"),
    };
    assert_eq!(rev_before, rev_after, "requests never mutate the lease");

    // Concurrent policy CAS converges: exactly one writer wins, the stale
    // writer conflicts with zero overwrite. Writer 1 moves Carol to Deny
    // (revision bump); writer 2 replays the stale revision and conflicts.
    let policy_rev = match call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatPolicyGet {
            project_id: w.project_a.as_str().to_owned(),
            channel_id: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => project.revision,
        other => panic!("expected policy, got {other:?}"),
    };
    let winner = call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatProjectPolicySet {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.carol.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: Some(policy_rev),
        },
    )
    .await;
    assert!(!is_denied(&winner));
    let loser = call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatProjectPolicySet {
            project_id: w.project_a.as_str().to_owned(),
            principal_id: w.carol.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: Some(policy_rev),
        },
    )
    .await;
    assert!(is_denied(&loser));
    assert_eq!(error_code(&loser), "chat_revision_conflict");
    // The winner's Deny stands (zero overwrite by the loser): Carol is
    // denied until Alice restores the grant at the current revision.
    let carol_denied = send(
        &w.daemon,
        "client-carol",
        &w.channel_a,
        "carol raced out",
        None,
    )
    .await;
    assert!(is_denied(&carol_denied));
    let current_rev = match call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatPolicyGet {
            project_id: w.project_a.as_str().to_owned(),
            channel_id: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => project.revision,
        other => panic!("expected policy, got {other:?}"),
    };
    assert!(!is_denied(
        &call(
            &w.daemon,
            "client-alice",
            CoreRequest::ChatProjectPolicySet {
                project_id: w.project_a.as_str().to_owned(),
                principal_id: w.carol.as_str().to_owned(),
                decision: Some(ChatPolicyDecisionDto::Allow),
                expected_revision: Some(current_rev),
            },
        )
        .await
    ));
    send_ok(
        &w.daemon,
        "client-carol",
        &w.channel_a,
        "carol after cas race",
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn secret_negative_census_across_trajectory() {
    let w = world().await;
    register_client(
        &w.daemon,
        "client-local",
        AuthenticatedPrincipal::local_owner("client-local"),
    );

    // Mint one device token for Bob through the LocalOwner surface.
    let (token_id, plaintext) = match call(
        &w.daemon,
        "client-local",
        CoreRequest::TeamTokenCreate {
            principal_id: w.bob.as_str().to_owned(),
            label: "m006-device".to_owned(),
            expires_at_ms: None,
            idempotency_key: None,
        },
    )
    .await
    {
        CoreResponse::TeamTokenCreated { token, plaintext } => (token.token_id, plaintext),
        other => panic!("expected token created, got {other:?}"),
    };
    assert!(!plaintext.is_empty());

    // Plaintext appears exactly once (in TeamTokenCreated) and never in
    // list responses, membership views, chat, control, or composing state.
    let list = call(
        &w.daemon,
        "client-local",
        CoreRequest::TeamTokenList {
            principal_id: w.bob.as_str().to_owned(),
        },
    )
    .await;
    assert!(!format!("{list:?}").contains(&plaintext));
    assert!(format!("{list:?}").contains(&token_id));
    let members = call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipList {
            project_id: w.project_a.as_str().to_owned(),
            limit: None,
        },
    )
    .await;
    assert!(!format!("{members:?}").contains(&plaintext));
    send_ok(&w.daemon, "client-alice", &w.channel_a, "census seed").await;
    let hist = call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatHistory {
            channel_id: w.channel_a.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(!format!("{hist:?}").contains(&plaintext));
    let sync = call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatSync {
            channel_id: w.channel_a.clone(),
            from_seq: 0,
            limit: None,
        },
    )
    .await;
    assert!(!format!("{sync:?}").contains(&plaintext));
    let composing = call(
        &w.daemon,
        "client-alice",
        CoreRequest::ChatComposingList {
            channel_id: w.channel_a.clone(),
        },
    )
    .await;
    assert!(!format!("{composing:?}").contains(&plaintext));
    let (_turn, _cancel, _steer) = install_alice_turn(&w, &w.session_a, "census").await;
    let control = call(
        &w.daemon,
        "client-alice",
        CoreRequest::SessionControlGet {
            session_id: w.session_a.clone(),
        },
    )
    .await;
    assert!(!format!("{control:?}").contains(&plaintext));

    // Chat bodies never leak into content-free composing snapshots.
    let marker = "m006-marker-body-9f31";
    send_ok(&w.daemon, "client-alice", &w.channel_a, marker).await;
    let composing2 = call(
        &w.daemon,
        "client-bob",
        CoreRequest::ChatComposingList {
            channel_id: w.channel_a.clone(),
        },
    )
    .await;
    assert!(!format!("{composing2:?}").contains(marker));
}

// ── §9: pre-corrective compatibility + LocalOwner solo ─────────────────

#[tokio::test(flavor = "current_thread")]
async fn pre_corrective_role_defaults_and_local_owner_solo() {
    // A pre-corrective-shaped database: ordinary roles, channels/sessions,
    // no policy overlay rows. Role defaults must hold as the compatibility
    // baseline (Viewer denied, Contributor allowed).
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    seed_session(team.pool(), &project, "sess-m006-compat").await;
    let owner = human_token(&team, "m006-c-owner", "c-owner").await;
    let owner_id = owner.principal_id().clone();
    team.create_membership(&project, &owner_id, ProjectRole::Owner)
        .await
        .unwrap();
    let viewer = human_token(&team, "m006-c-viewer", "c-viewer").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    let contrib = human_token(&team, "m006-c-contrib", "c-contrib").await;
    let contrib_id = contrib.principal_id().clone();
    team.create_membership(&project, &contrib_id, ProjectRole::Contributor)
        .await
        .unwrap();
    register_client(&daemon, "c-owner", owner);
    register_client(&daemon, "c-viewer", viewer);
    register_client(&daemon, "c-contrib", contrib);

    let channel = ensure_channel(&daemon, "c-owner", project.as_str(), None).await;
    let viewer_send = send(&daemon, "c-viewer", &channel, "viewer compat", None).await;
    assert!(is_denied(&viewer_send), "viewer default deny preserved");
    send_ok(&daemon, "c-contrib", &channel, "contrib compat").await;

    // LocalOwner solo mode: no team setup at all. A fresh catalog,
    // project registration, ordinary session, and chat all work through
    // the local boundary.
    let solo_pool = test_pool().await;
    let solo = CoreDaemon::new(Some(solo_pool.clone()), None, None);
    register_client(
        &solo,
        "client-local",
        AuthenticatedPrincipal::local_owner("client-local"),
    );
    let solo_dir = tempfile::tempdir().unwrap();
    let solo_root = solo_dir.path().display().to_string();
    let workspace_id = match call(
        &solo,
        "client-local",
        CoreRequest::WorkspaceRegister {
            root: solo_root.clone(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("localowner solo workspace register, got {other:?}"),
    };
    let project_id = match call(
        &solo,
        "client-local",
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: "solo".to_owned(),
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
        other => panic!("localowner solo project register, got {other:?}"),
    };
    let session_id = match call(
        &solo,
        "client-local",
        CoreRequest::SessionCreate {
            directory: solo_root,
            title: Some("solo".to_owned()),
            project_id: Some(project_id.clone()),
            workspace_id: Some(workspace_id),
        },
    )
    .await
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("localowner solo session create, got {other:?}"),
    };
    assert!(!session_id.is_empty());
    let channel = ensure_channel(&solo, "client-local", &project_id, None).await;
    send_ok(&solo, "client-local", &channel, "solo hello").await;
}

// ── Cross-project mutation probes (zero side effect) ───────────────────

#[tokio::test(flavor = "current_thread")]
async fn cross_project_mutations_fail_closed_with_zero_side_effect() {
    let w = world().await;
    let members_before = match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipList {
            project_id: w.project_b.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembershipList { memberships, .. } => memberships.len(),
        other => panic!("expected list, got {other:?}"),
    };
    let chat_before = history_len(&w.daemon, "client-alice", &w.channel_b).await;

    // Bob (A-only) attempts mutations scoped to B: membership admin,
    // chat send, control request, policy inspection.
    let probes = [
        CoreRequest::TeamMembershipAdd {
            project_id: w.project_b.as_str().to_owned(),
            principal_id: w.mallory.as_str().to_owned(),
            role: "viewer".to_owned(),
        },
        CoreRequest::ChatSend {
            channel_id: w.channel_b.clone(),
            body: "bob cross-project".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
        },
        CoreRequest::SessionControlRequest {
            session_id: w.session_b.clone(),
            message: None,
        },
        CoreRequest::ChatPolicyGet {
            project_id: w.project_b.as_str().to_owned(),
            channel_id: None,
        },
    ];
    for probe in probes {
        let resp = call(&w.daemon, "client-bob", probe).await;
        assert!(is_denied(&resp), "cross-project probe fails closed");
    }
    // Mallory's outsider probes fail identically closed.
    for probe in [
        CoreRequest::TeamMembershipList {
            project_id: w.project_b.as_str().to_owned(),
            limit: None,
        },
        CoreRequest::SessionControlGet {
            session_id: w.session_b.clone(),
        },
    ] {
        let resp = call(&w.daemon, "client-mallory", probe).await;
        assert!(is_denied(&resp));
    }

    // Zero side effect: membership rows and B chat unchanged.
    match call(
        &w.daemon,
        "client-alice",
        CoreRequest::TeamMembershipList {
            project_id: w.project_b.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::TeamMembershipList { memberships, .. } => {
            assert_eq!(memberships.len(), members_before)
        }
        other => panic!("expected list, got {other:?}"),
    }
    assert_eq!(
        history_len(&w.daemon, "client-alice", &w.channel_b).await,
        chat_before
    );
}
