//! Presence and Observation M001 — project-scoped presence leases.
//!
//! Boundary proof for the plan: daemon-owned ephemeral leases with
//! heartbeat/idle/expiry/reconnect and privacy semantics. The principal
//! always comes from transport authority; snapshots aggregate multiple
//! clients/sessions deterministically; unauthorized projects are
//! indistinguishable from absent; expiry never deletes session history;
//! high churn stays bounded with a single cleanup entry point and no
//! task-per-lease; restart clears without fabrication; stale generations
//! cannot resurrect.

use std::time::{Duration, Instant};

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    CoreRequest, CoreResponse, PresenceActivityDto, PresenceHeartbeatRequestDto,
};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::presence::{PresenceActivity, PresenceConfig, PresenceError, PresenceService};
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

fn heartbeat_request(
    project_id: &str,
    session_id: Option<&str>,
    activity: PresenceActivityDto,
    generation: u64,
) -> CoreRequest {
    CoreRequest::PresenceHeartbeat {
        request: PresenceHeartbeatRequestDto {
            project_id: project_id.to_owned(),
            session_id: session_id.map(str::to_owned),
            activity,
            connection_generation: generation,
        },
    }
}

async fn heartbeat_as(
    daemon: &CoreDaemon,
    client: &str,
    project_id: &str,
    session_id: Option<&str>,
    activity: PresenceActivityDto,
    generation: u64,
) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-hb-{client}-{}", uuid::Uuid::new_v4()),
            heartbeat_request(project_id, session_id, activity, generation),
        ),
        client,
    ))
    .await
    .unwrap()
}

async fn snapshot_as(daemon: &CoreDaemon, client: &str, project_id: &str) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-snap-{client}-{}", uuid::Uuid::new_v4()),
            CoreRequest::PresenceSnapshotGet {
                project_id: project_id.to_owned(),
            },
        ),
        client,
    ))
    .await
    .unwrap()
}

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

// ── Service-level state machine ──────────────────────────────────────

#[test]
fn service_heartbeat_expiry_idle_lifecycle() {
    let presence = PresenceService::new(PresenceConfig::default());
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let start = Instant::now();
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            start,
            1_000,
        )
        .unwrap();
    // Live immediately.
    assert_eq!(presence.live_leases(), 1);
    // Past idle (30s default) but before expiry (90s): idle, present.
    let idle_at = start + Duration::from_secs(31);
    let snapshot = presence.snapshot(&project, idle_at, 2_000);
    assert_eq!(snapshot.principals.len(), 1);
    assert_eq!(snapshot.principals[0].activity, PresenceActivityDto::Idle);
    // Past expiry: single bounded cleanup reclaims.
    let expired_at = start + Duration::from_secs(91);
    assert_eq!(presence.evict_expired(expired_at), 1);
    assert_eq!(presence.live_leases(), 0);
    assert!(presence
        .snapshot(&project, expired_at, 3_000)
        .principals
        .is_empty());
}

#[test]
fn service_two_clients_one_principal_aggregate() {
    let presence = PresenceService::default();
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let now = Instant::now();
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            now,
            1_000,
        )
        .unwrap();
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-2",
            Some("session-2"),
            PresenceActivity::Observing,
            1,
            now,
            1_001,
        )
        .unwrap();
    let snapshot = presence.snapshot(&project, now, 1_002);
    assert_eq!(snapshot.principals.len(), 1);
    let row = &snapshot.principals[0];
    assert_eq!(row.client_count, 2);
    assert_eq!(row.session_ids, vec!["session-1", "session-2"]);
    assert_eq!(row.activity, PresenceActivityDto::Active);
}

#[test]
fn service_several_sessions_activity_aggregation() {
    let presence = PresenceService::default();
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let now = Instant::now();
    for (session, activity) in [
        ("session-1", PresenceActivity::Observing),
        ("session-2", PresenceActivity::Active),
        ("session-3", PresenceActivity::AgentRunning),
    ] {
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some(session),
                activity,
                1,
                now,
                1_000,
            )
            .unwrap();
    }
    let snapshot = presence.snapshot(&project, now, 1_000);
    assert_eq!(
        snapshot.principals[0].activity,
        PresenceActivityDto::AgentRunning
    );
    assert_eq!(snapshot.principals[0].session_ids.len(), 3);
}

#[test]
fn service_disconnect_reconnect_without_duplicates() {
    let presence = PresenceService::default();
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let now = Instant::now();
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            now,
            1_000,
        )
        .unwrap();
    assert_eq!(
        presence.disconnect_client_in_project(&project, &principal, "client-1"),
        1
    );
    assert_eq!(presence.live_leases(), 0);
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            2,
            now,
            2_000,
        )
        .unwrap();
    assert_eq!(presence.live_leases(), 1);
}

#[test]
fn service_stale_generation_cannot_resurrect() {
    let presence = PresenceService::default();
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let start = Instant::now();
    presence
        .heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            2,
            start,
            1_000,
        )
        .unwrap();
    let expired_at = start + Duration::from_secs(91);
    assert_eq!(presence.evict_expired(expired_at), 1);
    assert_eq!(
        presence.heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            expired_at,
            2_000,
        ),
        Err(PresenceError::StaleGeneration)
    );
    assert_eq!(presence.live_leases(), 0);
}

#[test]
fn service_restart_clears_without_fabrication() {
    let presence = PresenceService::default();
    let project = ProjectId::parse("project-1").unwrap();
    let principal = PrincipalId::parse("principal-1").unwrap();
    let now = Instant::now();
    presence
        .heartbeat(
            project.clone(),
            principal,
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            now,
            1_000,
        )
        .unwrap();
    presence.clear();
    assert_eq!(presence.live_leases(), 0);
    assert!(presence
        .snapshot(&project, now, 2_000)
        .principals
        .is_empty());
}

#[test]
fn service_high_churn_stays_bounded() {
    let presence = PresenceService::new(PresenceConfig {
        max_contributions: 8,
        max_projects: 2,
        max_principals_per_project: 4,
        max_sessions_per_principal: 2,
        ..PresenceConfig::default()
    });
    let now = Instant::now();
    let mut rejected = 0;
    for i in 0..64 {
        let project = ProjectId::parse(&format!("project-{}", i % 4)).unwrap();
        let principal = PrincipalId::parse(&format!("principal-{}", i % 8)).unwrap();
        let client = format!("client-{}", i % 8);
        let session = format!("session-{}", i % 4);
        match presence.heartbeat(
            project,
            principal,
            &client,
            Some(session.as_str()),
            PresenceActivity::Active,
            1,
            now,
            1_000 + i as i64,
        ) {
            Ok(_) => {}
            Err(PresenceError::Capacity) => rejected += 1,
            Err(other) => panic!("unexpected presence error: {other:?}"),
        }
    }
    assert!(rejected > 0, "churn must hit the bound");
    assert!(presence.live_leases() <= 8);
    // Exactly one cleanup entry point exists (no task-per-lease): a
    // single scan reclaims everything after expiry.
    presence.evict_expired(now + Duration::from_secs(120));
    assert_eq!(presence.live_leases(), 0);
}

// ── Daemon boundary: auth, privacy, protocol ─────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_presence_capabilities_negotiate_bounds() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool), None, None);
    let response = Box::pin(daemon.handle_request(new_request(
        "req-presence-caps".to_owned(),
        CoreRequest::PresenceCapabilities,
    )))
    .await
    .unwrap();
    match response {
        CoreResponse::PresenceCapabilities { capabilities } => {
            assert!(capabilities.supported);
            assert_eq!(capabilities.protocol_version, 1);
            assert!(capabilities.lease_ttl_secs > 0);
            assert!(capabilities.max_contributions > 0);
        }
        other => panic!("expected presence capabilities, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_heartbeat_and_snapshot_round_trip() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-member").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-member", member);

    let ack = heartbeat_as(
        &daemon,
        "client-member",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        1,
    )
    .await;
    match ack {
        CoreResponse::PresenceHeartbeatAck {
            project_id,
            expires_at_ms,
        } => {
            assert_eq!(project_id, project.as_str());
            assert!(expires_at_ms > 0);
        }
        other => panic!("expected heartbeat ack, got {other:?}"),
    }
    match snapshot_as(&daemon, "client-member", project.as_str()).await {
        CoreResponse::PresenceSnapshot { snapshot } => {
            assert_eq!(snapshot.project_id, project.as_str());
            assert_eq!(snapshot.principals.len(), 1);
            assert_eq!(snapshot.principals[0].session_ids, vec!["session-1"]);
        }
        other => panic!("expected presence snapshot, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_unauthorized_project_is_indistinguishable_from_absent() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-member").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-member", member);
    register_client(&daemon, "client-outsider", outsider);

    // Member seeds one lease so the project is non-empty for members.
    match heartbeat_as(
        &daemon,
        "client-member",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        1,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("member heartbeat failed: {other:?}"),
    }

    // Outsider heartbeat and snapshot deny as not-found.
    assert_eq!(
        error_code(
            &heartbeat_as(
                &daemon,
                "client-outsider",
                project.as_str(),
                Some("session-9"),
                PresenceActivityDto::Active,
                1,
            )
            .await
        ),
        "project_not_found"
    );
    assert_eq!(
        error_code(&snapshot_as(&daemon, "client-outsider", project.as_str()).await),
        "project_not_found"
    );
    // A genuinely absent project denies identically: no existence,
    // membership, collaborator, or activity signal leaks.
    let absent = ProjectId::new();
    assert_eq!(
        error_code(&snapshot_as(&daemon, "client-outsider", absent.as_str()).await),
        "project_not_found"
    );
    assert_eq!(
        error_code(&snapshot_as(&daemon, "client-member", absent.as_str()).await),
        "project_not_found"
    );
    // Zero side effect: the outsider heartbeat created no lease.
    match snapshot_as(&daemon, "client-member", project.as_str()).await {
        CoreResponse::PresenceSnapshot { snapshot } => {
            assert_eq!(snapshot.principals.len(), 1);
        }
        other => panic!("member snapshot failed: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_request_dtos_carry_no_authority() {
    for request in [
        heartbeat_request(
            "project-1",
            Some("session-1"),
            PresenceActivityDto::Active,
            1,
        ),
        CoreRequest::PresenceSnapshotGet {
            project_id: "project-1".to_owned(),
        },
    ] {
        let value = serde_json::to_value(&request).expect("serialize presence request");
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
                "presence DTO must not carry {forbidden}: {request:?}"
            );
        }
        // Nested heartbeat body carries only locators + activity.
        if let CoreRequest::PresenceHeartbeat { request } = &request {
            let body = serde_json::to_value(request).expect("heartbeat body");
            let body_object = body.as_object().expect("heartbeat object");
            assert!(body_object.contains_key("project_id"));
            assert!(!body_object.contains_key("principal_id"));
            assert!(!body_object.contains_key("client_id"));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_disconnect_expires_without_deleting_history() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-member").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-member", member);

    // Seed a durable session row so we can prove expiry does not
    // delete session history.
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
    .bind("session-1")
    .bind(project.as_str())
    .execute(&pool)
    .await
    .unwrap();

    match heartbeat_as(
        &daemon,
        "client-member",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        1,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("heartbeat failed: {other:?}"),
    }
    assert_eq!(daemon.presence.live_leases(), 1);
    daemon.note_client_disconnected("client-member");
    assert_eq!(daemon.presence.live_leases(), 0);
    // Session history survives presence expiry.
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM session WHERE id = 'session-1'")
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(
        row.is_some(),
        "lease expiry must not delete session history"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_restart_clears_without_fabrication() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-member").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-member", member);
    match heartbeat_as(
        &daemon,
        "client-member",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        1,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("heartbeat failed: {other:?}"),
    }
    assert_eq!(daemon.presence.live_leases(), 1);
    // Daemon restart clears all leases; rebuild comes only from active
    // connections afterwards.
    daemon.presence.clear();
    match snapshot_as(&daemon, "client-member", project.as_str()).await {
        CoreResponse::PresenceSnapshot { snapshot } => {
            assert!(snapshot.principals.is_empty());
        }
        other => panic!("snapshot after restart failed: {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_stale_generation_heartbeat_is_rejected() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-member").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-member", member);
    match heartbeat_as(
        &daemon,
        "client-member",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        2,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("gen2 heartbeat failed: {other:?}"),
    }
    assert_eq!(
        error_code(
            &heartbeat_as(
                &daemon,
                "client-member",
                project.as_str(),
                Some("session-1"),
                PresenceActivityDto::Active,
                1,
            )
            .await
        ),
        "presence_stale_generation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_two_clients_one_principal_aggregate_over_protocol() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = human_token(&team, "Member", "client-a").await;
    let member_record = team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &member_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    // One principal, two transport connections.
    register_client(&daemon, "client-a", member.clone());
    register_client(&daemon, "client-b", member);
    match heartbeat_as(
        &daemon,
        "client-a",
        project.as_str(),
        Some("session-1"),
        PresenceActivityDto::Active,
        1,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("client-a heartbeat failed: {other:?}"),
    }
    match heartbeat_as(
        &daemon,
        "client-b",
        project.as_str(),
        Some("session-2"),
        PresenceActivityDto::Observing,
        1,
    )
    .await
    {
        CoreResponse::PresenceHeartbeatAck { .. } => {}
        other => panic!("client-b heartbeat failed: {other:?}"),
    }
    match snapshot_as(&daemon, "client-a", project.as_str()).await {
        CoreResponse::PresenceSnapshot { snapshot } => {
            assert_eq!(snapshot.principals.len(), 1);
            assert_eq!(snapshot.principals[0].client_count, 2);
            assert_eq!(
                snapshot.principals[0].session_ids,
                vec!["session-1", "session-2"]
            );
        }
        other => panic!("snapshot failed: {other:?}"),
    }
}
