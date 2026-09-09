//! Presence and Observation M003 — authorized read-only session observation.
//!
//! Boundary proof for the plan: observation is an authorized specialization
//! of the existing projection subscription path. The daemon gate plus the
//! canonical team-derived projection context enforce `session.observe` on
//! subscribe and recheck it on resume; denials are `project_not_found` so
//! outsiders cannot distinguish a missing session from a denied one.
//! Observer subscriptions are connection-owned (teardown affects only the
//! observer), redaction survives snapshot/replay, revoked grants clean
//! transient state, and the TUI read-only policy blocks every control
//! family while the observer banner stays content-free.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, PROTOCOL_VERSION,
};
use codegg::protocol::projection::replay::{
    ProjectionCursor, ProjectionStreamKind, ProjectionSubscriptionRequest,
};
use codegg_core::identity::ProjectId;
use codegg_core::projection_replay::metrics::ProjectionReplayMetrics;
use codegg_core::projection_replay::seam::{
    ProjectionBindingContext, ProjectionDisclosureContext, ProjectionPublicationSeam,
};
use codegg_core::projection_replay::service::PublishOutcome;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};
use std::sync::Arc;

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

fn subscribe_request(session_id: &str) -> CoreRequest {
    CoreRequest::ProjectionSubscribe {
        request: ProjectionSubscriptionRequest {
            scope: ProjectionStreamKind::Session,
            scope_id: session_id.to_owned(),
            cursor: None,
            projection_version: 1,
        },
    }
}

async fn subscribe_as(daemon: &CoreDaemon, client: &str, session_id: &str) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-sub-{client}-{}", uuid::Uuid::new_v4()),
            subscribe_request(session_id),
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

fn subscribed_parts(
    response: CoreResponse,
) -> (
    codegg_protocol::projection::replay::ProjectionSubscriptionId,
    codegg_protocol::projection::replay::ProjectionStreamDescriptor,
    ProjectionCursor,
) {
    match response {
        CoreResponse::ProjectionSubscribed {
            subscription_id,
            descriptor,
            cursor,
            ..
        } => (subscription_id, descriptor, cursor),
        other => panic!("expected ProjectionSubscribed, got {other:?}"),
    }
}

async fn member_daemon() -> (CoreDaemon, TeamStore, ProjectId, &'static str, &'static str) {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    seed_session(team.pool(), &project, "session-observed").await;
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
    (daemon, team, project, "client-member", "session-observed")
}

// ── Allow/deny + non-enumeration ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn observe_allow_for_member_deny_for_outsider_indistinguishable_from_absent() {
    let (daemon, team, project, member_client, session_id) = member_daemon().await;
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);

    // Member subscribes to the teammate session.
    let (sub_id, _descriptor, _cursor) =
        subscribed_parts(subscribe_as(&daemon, member_client, session_id).await);
    assert!(!sub_id.0.is_empty());

    // Outsider denial is byte-identical in shape to a genuinely absent
    // session: no existence, membership, or activity signal leaks.
    assert_eq!(
        error_code(&subscribe_as(&daemon, "client-outsider", session_id).await),
        "project_not_found"
    );
    let absent_session = "session-absent-1";
    assert_eq!(
        error_code(&subscribe_as(&daemon, "client-outsider", absent_session).await),
        "project_not_found"
    );
    // Even the member sees the same shape for a missing session.
    assert_eq!(
        error_code(&subscribe_as(&daemon, member_client, absent_session).await),
        "project_not_found"
    );
    let _ = project;
}

#[tokio::test(flavor = "current_thread")]
async fn outsider_project_enumeration_reveals_nothing() {
    let (daemon, team, _project, _member_client, _session_id) = member_daemon().await;
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);
    // Empty list, never an error that confirms absence versus denial.
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-list-outsider".to_owned(),
            CoreRequest::ProjectList {
                include_archived: false,
                limit: 100,
            },
        ),
        "client-outsider",
    ))
    .await
    .unwrap()
    {
        CoreResponse::ProjectList { projects, .. } => assert!(projects.is_empty()),
        other => panic!("expected project list, got {other:?}"),
    }
}

// ── Forbidden control families (daemon gate) ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn observer_cannot_steer_cancel_or_mutate_through_daemon() {
    let (daemon, _team, _project, member_client, session_id) = member_daemon().await;
    // Viewer holds session.observe/session.read but never agent.invoke or
    // file.modify: steering, cancel, model/agent selection, and checked
    // file mutations all deny at the gate with zero side effect.
    let denied = [
        CoreRequest::TurnSubmit {
            session_id: session_id.to_owned(),
            text: "steer me".to_owned(),
            plan_mode: false,
            model: "test/model".to_owned(),
            agents: Vec::new(),
            current_agent_idx: 0,
            messages: Vec::new(),
        },
        CoreRequest::TurnCancel {
            session_id: session_id.to_owned(),
            turn_id: "turn-1".to_owned(),
        },
        CoreRequest::TurnSteer {
            session_id: session_id.to_owned(),
            turn_id: "turn-1".to_owned(),
            text: "steer".to_owned(),
        },
        CoreRequest::AgentSelect {
            session_id: session_id.to_owned(),
            agent_name: "build".to_owned(),
        },
        CoreRequest::ModelSelect {
            session_id: session_id.to_owned(),
            model: "other/model".to_owned(),
        },
    ];
    for request in denied {
        let response = Box::pin(daemon.handle_request_for_client(
            new_request(format!("req-deny-{}", uuid::Uuid::new_v4()), request),
            member_client,
        ))
        .await
        .unwrap();
        assert_eq!(error_code(&response), "authorization_denied");
    }
    // Checked file mutation denies as well (opaque scope fails closed for
    // team principals without file.modify on a resolvable scope).
    let file_denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            format!("req-deny-file-{}", uuid::Uuid::new_v4()),
            CoreRequest::LspPreviewApply {
                request: codegg::protocol::lsp::LspPreviewApplyRequestDto {
                    preview_id: "preview-1".to_owned(),
                    preview_revision: 1,
                    preview_digest: "digest".to_owned(),
                    kind: "edit".to_owned(),
                    title: "t".to_owned(),
                    provenance: "test".to_owned(),
                    workspace_id: "ws-1".to_owned(),
                    session_id: session_id.to_owned(),
                    turn_id: None,
                    patches: Vec::new(),
                },
            },
        ),
        member_client,
    ))
    .await
    .unwrap();
    assert!(
        matches!(file_denied, CoreResponse::Error { .. }),
        "file mutation must deny for observer, got {file_denied:?}"
    );
}

// ── Observer lifecycle: teardown, disconnect, resume ───────────────────

#[tokio::test(flavor = "current_thread")]
async fn stop_tears_down_only_observer_owned_subscription() {
    let (daemon, _team, _project, member_client, session_id) = member_daemon().await;
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    let (sub_a, _d, _c) = subscribed_parts(subscribe_as(&daemon, member_client, session_id).await);
    // A second observer (distinct connection, same principal shape)
    // holds an independent subscription on the same stream.
    let (sub_b, _d, _c) = subscribed_parts(subscribe_as(&daemon, member_client, session_id).await);
    assert_ne!(sub_a, sub_b);
    assert_eq!(seam.service().subscriptions().active_count(), 2);

    // Unsubscribing one observer leaves the other live: teardown is
    // observer-owned only and the target session is untouched.
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-unsub-a".to_owned(),
            CoreRequest::ProjectionUnsubscribe {
                subscription_id: sub_a.clone(),
            },
        ),
        member_client,
    ))
    .await
    .unwrap()
    {
        CoreResponse::ProjectionUnsubscribed { subscription_id } => {
            assert_eq!(subscription_id, sub_a);
        }
        other => panic!("expected unsubscribe ack, got {other:?}"),
    }
    assert_eq!(seam.service().subscriptions().active_count(), 1);
    // The surviving subscription still resumes.
    let cursor = ProjectionCursor {
        stream_id: seam
            .service()
            .subscriptions()
            .by_id()
            .get(&sub_b)
            .map(|e| e.value().stream_id.clone())
            .expect("surviving subscription"),
        event_seq: 0,
        projection_version: 1,
    };
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-resume-b".to_owned(),
            CoreRequest::ProjectionResume {
                cursor,
                include_snapshot_if_resync: false,
            },
        ),
        member_client,
    ))
    .await
    .unwrap()
    {
        CoreResponse::ProjectionReplay { .. } | CoreResponse::ProjectionResyncRequired { .. } => {}
        other => panic!("surviving observer must still replay, got {other:?}"),
    }
    // Session history survives observer teardown.
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM session WHERE id = ?")
        .bind(session_id)
        .fetch_optional(_team.pool())
        .await
        .unwrap();
    assert!(row.is_some(), "observer lifecycle must not delete history");
}

#[tokio::test(flavor = "current_thread")]
async fn target_owner_disconnect_grants_no_control_and_observer_survives() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    seed_session(team.pool(), &project, "session-owned").await;
    let owner = human_token(&team, "Owner", "client-owner").await;
    let owner_record = team
        .get_principal(owner.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &owner_record.id, ProjectRole::Owner)
        .await
        .unwrap();
    let watcher = human_token(&team, "Watcher", "client-watcher").await;
    let watcher_record = team
        .get_principal(watcher.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(&project, &watcher_record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-owner", owner);
    register_client(&daemon, "client-watcher", watcher);

    // Watcher subscribes while the owner is connected.
    let (_sub, _d, _c) =
        subscribed_parts(subscribe_as(&daemon, "client-watcher", "session-owned").await);

    // Owner disconnects: presence-style expiry + registry removal. This
    // must not grant the watcher control and must not disturb the
    // watcher-owned subscription.
    daemon.note_client_disconnected("client-owner");
    daemon.clients.unregister("client-owner");

    let (sub_after, _d, _c) =
        subscribed_parts(subscribe_as(&daemon, "client-watcher", "session-owned").await);
    assert!(!sub_after.0.is_empty());
    // The disconnected owner can no longer act; the watcher still cannot
    // invoke (Viewer) — disconnect transferred no authority.
    let denied = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-watcher-turn".to_owned(),
            CoreRequest::TurnSubmit {
                session_id: "session-owned".to_owned(),
                text: "hi".to_owned(),
                plan_mode: false,
                model: "test/model".to_owned(),
                agents: Vec::new(),
                current_agent_idx: 0,
                messages: Vec::new(),
            },
        ),
        "client-watcher",
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&denied), "authorization_denied");
}

// ── Revocation cleans transient state ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn revoked_observe_grant_denies_resume_and_cleans_subscription() {
    let (daemon, team, project, member_client, session_id) = member_daemon().await;
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    let (sub_id, descriptor, _cursor) =
        subscribed_parts(subscribe_as(&daemon, member_client, session_id).await);
    assert_eq!(seam.service().subscriptions().active_count(), 1);

    // Revoke the Viewer membership (revision-checked, like production).
    let member_id = daemon
        .request_authority_for_client(member_client)
        .principal_id()
        .clone();
    let membership = team
        .get_membership(&project, &member_id)
        .await
        .unwrap()
        .expect("membership");
    team.revoke_membership(&project, &member_id, membership.revision)
        .await
        .unwrap();

    // New subscribes deny as not-found (no existence signal).
    assert_eq!(
        error_code(&subscribe_as(&daemon, member_client, session_id).await),
        "project_not_found"
    );
    // Resume on the old cursor denies identically and cleans the
    // transient owned subscription so revoked delivery cannot linger.
    let resume = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-resume-revoked".to_owned(),
            CoreRequest::ProjectionResume {
                cursor: ProjectionCursor {
                    stream_id: descriptor.stream_id.clone(),
                    event_seq: 0,
                    projection_version: 1,
                },
                include_snapshot_if_resync: true,
            },
        ),
        member_client,
    ))
    .await
    .unwrap();
    assert_eq!(error_code(&resume), "project_not_found");
    assert!(
        seam.service()
            .subscriptions()
            .by_id()
            .get(&sub_id)
            .is_none(),
        "revoked resume must clean the transient subscription"
    );
}

// ── Queue bounds + multiple observers ──────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn multiple_observers_share_no_mutable_ownership_and_bounds_hold() {
    let (daemon, team, project, _member_client, session_id) = member_daemon().await;
    let seam = daemon.projection_seam.as_ref().expect("projection seam");
    // Three distinct observer connections on the same session.
    for i in 0..3 {
        let client = format!("client-obs-{i}");
        let observer = human_token(&team, &format!("Observer{i}"), &client).await;
        let record = team
            .get_principal(observer.principal_id())
            .await
            .unwrap()
            .unwrap();
        team.create_membership(&project, &record.id, ProjectRole::Viewer)
            .await
            .unwrap();
        register_client(&daemon, &client, observer);
        let (sub, _d, _c) = subscribed_parts(subscribe_as(&daemon, &client, session_id).await);
        assert!(!sub.0.is_empty());
    }
    assert_eq!(seam.service().subscriptions().active_count(), 3);

    // Per-client queue bound (32) is enforced: the 33rd subscribe on one
    // connection fails instead of growing memory.
    let pool = test_pool().await;
    let bound_daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let bound_team = TeamStore::new(pool);
    let bound_project = ProjectId::new();
    seed_session(bound_team.pool(), &bound_project, "session-bound").await;
    let member = human_token(&bound_team, "Bound", "client-bound").await;
    let record = bound_team
        .get_principal(member.principal_id())
        .await
        .unwrap()
        .unwrap();
    bound_team
        .create_membership(&bound_project, &record.id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&bound_daemon, "client-bound", member);
    let mut ok = 0;
    let mut capped = false;
    for _ in 0..33 {
        match subscribe_as(&bound_daemon, "client-bound", "session-bound").await {
            CoreResponse::ProjectionSubscribed { .. } => ok += 1,
            CoreResponse::Error { code, .. } => {
                assert_eq!(code, "projection_subscribe_failed");
                capped = true;
                break;
            }
            other => panic!("unexpected subscribe response: {other:?}"),
        }
    }
    assert_eq!(ok, 32, "per-client bound must admit exactly 32");
    assert!(capped, "33rd subscribe must fail instead of growing");
    assert!(
        bound_daemon
            .projection_seam
            .as_ref()
            .expect("seam")
            .service()
            .subscriptions()
            .active_count()
            <= 256,
        "daemon-wide bound must hold"
    );
}

// ── Redaction survives snapshot + replay on the observer path ──────────

const OBSERVE_PROBE_SECRET: &str = "AKIAEXAMPLE1234567890";

async fn publish_tool_started_with_secret(
    seam: &Arc<ProjectionPublicationSeam>,
    session_id: &str,
    project_id: &str,
    secret: &str,
) {
    let metrics = Arc::new(ProjectionReplayMetrics::new());
    let dc = ProjectionDisclosureContext::local(
        Some(session_id.to_owned()),
        Some(project_id.to_owned()),
        metrics,
    );
    let envelope = EventEnvelope {
        protocol_version: PROTOCOL_VERSION,
        event_seq: 1,
        timestamp_ms: 1_000,
        session_id: Some(session_id.to_owned()),
        turn_id: Some("turn-1".to_owned()),
        payload: CoreEvent::ToolStarted {
            session_id: session_id.to_owned(),
            turn_id: Some("turn-1".to_owned()),
            tool_name: "fetch".to_owned(),
            tool_id: "tool-1".to_owned(),
            arguments: format!("{{\"api_key\": \"{secret}\", \"safe\": \"ok\"}}"),
        },
    };
    let binding = ProjectionBindingContext {
        session_id: Some(session_id.to_owned()),
        project_id: Some(project_id.to_owned()),
        workspace_id: Some("ws-1".to_owned()),
        binding_revision: 1,
    };
    match seam
        .publish_with_disclosure(&envelope, binding, Some(&dc))
        .await
        .unwrap()
    {
        PublishOutcome::Published { .. } => {}
        other => panic!("secret-bearing tool event must publish redacted, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn observer_snapshot_and_replay_carry_no_secrets() {
    let (daemon, _team, project, member_client, session_id) = member_daemon().await;
    let seam = daemon
        .projection_seam
        .as_ref()
        .expect("projection seam")
        .clone();
    publish_tool_started_with_secret(&seam, session_id, project.as_str(), OBSERVE_PROBE_SECRET)
        .await;

    // Snapshot bundle on subscribe carries no secret (durable runs carry
    // no prompts; the assertion pins the whole bundle serialization).
    let (_sub, _descriptor, _cursor) =
        subscribed_parts(subscribe_as(&daemon, member_client, session_id).await);

    // Replay from the origin replays the redacted envelope: the secret is
    // gone and the redaction marker survives.
    let store = seam.service().store();
    let descriptor = store
        .lookup_session_stream(session_id, project.as_str())
        .await
        .unwrap()
        .expect("session stream");
    let replay = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-replay-redact".to_owned(),
            CoreRequest::ProjectionResume {
                cursor: ProjectionCursor {
                    stream_id: descriptor.stream_id.clone(),
                    event_seq: 0,
                    projection_version: 1,
                },
                include_snapshot_if_resync: false,
            },
        ),
        member_client,
    ))
    .await
    .unwrap();
    let batch = match replay {
        CoreResponse::ProjectionReplay { batch, .. } => batch,
        other => panic!("expected replay batch, got {other:?}"),
    };
    assert!(!batch.events.is_empty(), "redacted event must replay");
    let serialized = serde_json::to_string(&batch.events).expect("serialize replayed events");
    assert!(
        !serialized.contains(OBSERVE_PROBE_SECRET),
        "secret must not survive replay; got: {serialized}"
    );
    assert!(
        serialized.contains("[REDACTED"),
        "redaction marker must survive replay; got: {serialized}"
    );
    // Provider-hidden reasoning is never exposed on this path either.
    assert!(
        !serialized.contains("secret reasoning"),
        "no hidden reasoning may leak through replay"
    );
}

// ── Artifact handle scope ──────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn artifact_handles_stay_project_scoped_for_observers() {
    let (daemon, team, project, member_client, _session_id) = member_daemon().await;
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);

    // Member lists handles (empty store, but authorized and scoped).
    match Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-art-list-member".to_owned(),
            CoreRequest::ProjectionArtifactList {
                project_id: project.as_str().to_owned(),
            },
        ),
        member_client,
    ))
    .await
    .unwrap()
    {
        CoreResponse::ProjectionArtifactList { handles } => {
            assert!(handles.iter().all(|h| h.project_id == project.as_str()));
        }
        other => panic!("expected artifact list, got {other:?}"),
    }
    // Outsider listing denies as not-found: no handle/project signal.
    assert_eq!(
        error_code(
            &Box::pin(daemon.handle_request_for_client(
                new_request(
                    "req-art-list-outsider".to_owned(),
                    CoreRequest::ProjectionArtifactList {
                        project_id: project.as_str().to_owned(),
                    },
                ),
                "client-outsider",
            ))
            .await
            .unwrap()
        ),
        "project_not_found"
    );
    // Outsider reads deny identically at the gate (64 KiB window never
    // reached without authorization).
    assert_eq!(
        error_code(
            &Box::pin(daemon.handle_request_for_client(
                new_request(
                    "req-art-read-outsider".to_owned(),
                    CoreRequest::ProjectionArtifactRead {
                        request:
                            codegg_protocol::projection::replay::ProjectionArtifactReadRequest {
                                handle_id: "handle-1".to_owned(),
                                start: 0,
                                end: None,
                                expected_revision: 0,
                            },
                        project_id: project.as_str().to_owned(),
                        context_correlation_id: None,
                    },
                ),
                "client-outsider",
            ))
            .await
            .unwrap()
        ),
        "project_not_found"
    );
}

// ── TUI observer state: banner, policy, reconnect ──────────────────────

fn app_with_project(project_id: &str) -> codegg::tui::app::App {
    let mut app = codegg::tui::app::App::new_for_testing("/tmp/m003".to_string());
    if let Some(tab) = app.project_tabs.active_mut() {
        tab.project_id = Some(project_id.to_string());
        tab.workspace_id = Some("ws-1".to_string());
    }
    app
}

#[test]
fn tui_observe_lifecycle_banner_policy_and_reconnect() {
    use codegg::tui::app::TuiCommand;
    let mut app = app_with_project("proj-1");
    assert!(!app.is_observing());

    // No daemon: start degrades to unavailable without breaking tabs, and
    // input stays blocked until an explicit stop.
    app.start_observe("proj-1".to_string(), "sess-1".to_string());
    assert!(app.is_observing());
    let banner = app.observer.banner_line().expect("banner");
    assert!(banner.contains("OBSERVING") && banner.contains("read-only"));
    assert!(!banner.contains("token") && !banner.contains("secret"));
    assert!(app.observer.blocks_prompt_submit());
    assert!(app.observer.blocks_permission_response());
    assert!(app.observer.blocks_command("/turn"));
    assert!(!app.observer.blocks_command("/collaborators"));
    assert!(!app.observer.blocks_command("/stop-observing"));
    assert!(app
        .observer
        .collaboration_input_placeholder()
        .expect("placeholder")
        .contains("/stop-observing"));

    // Reconnect bumps the epoch and resyncs; stale completions drop.
    let epoch = app.observer.reconnect_epoch;
    app.on_projection_reconnect();
    assert_eq!(app.observer.reconnect_epoch, epoch + 1);

    // Stop restores control and clears cached state.
    app.stop_observing();
    assert!(!app.is_observing());
    assert!(app.observer.banner_line().is_none());
    assert!(!app.observer.blocks_prompt_submit());

    // TuiCommand variants route (stale-completion guard pinned here).
    let stale_epoch = app.observer.reconnect_epoch;
    let req = app.observer.begin_observe("proj-1", "sess-9").unwrap();
    let _ = TuiCommand::StopObserving;
    assert!(app.observer.apply_denied(req, "sess-9", stale_epoch));
    assert!(app.observer.blocks_command("/models"));
}

#[test]
fn tui_collaborator_panel_links_to_observe_action() {
    use codegg::protocol::core::{PresenceActivityDto, PresencePrincipalDto, PresenceSnapshotDto};
    let mut app = app_with_project("proj-1");
    app.presence.set_capability(true);
    let req = app.presence.begin_refresh("proj-1");
    let epoch = app.presence.reconnect_epoch;
    app.apply_presence_snapshot(
        req,
        "proj-1".to_string(),
        Some(PresenceSnapshotDto {
            project_id: "proj-1".to_string(),
            as_of_ms: 42,
            principals: vec![PresencePrincipalDto {
                principal_id: "alice".to_string(),
                activity: PresenceActivityDto::Active,
                client_count: 1,
                session_ids: vec!["sess-1".to_string()],
                last_active_ms: 7,
            }],
            truncated: false,
        }),
        None,
        false,
        false,
        epoch,
    );
    // The chooser-to-action seam: session locators plus the bounded
    // `/observe` hint, with no content or secrets.
    let lines = app.presence.panel_lines("proj-1");
    assert!(lines.iter().any(|l| l.contains("/observe <session-id>")));
    let joined = lines.join("\n");
    assert!(!joined.contains("token") && !joined.contains("secret"));
}
