//! Project Collaboration M001 — project channel, message, and sync contract.
//!
//! Boundary proof for the plan: daemon-owned durable channels/messages with
//! replies/threads, mentions, typed references, edits/redactions, read
//! markers, ephemeral composing state, retention, and bounded idempotent
//! incremental sync. Project authorization (`project.chat`) guards every
//! operation; denials are indistinguishable from absent projects; retries
//! converge on idempotency keys; composing expires and drops on restart;
//! secrets are redacted before durable write; chat storage stays separate
//! from the audit store; free text has no execution semantics.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    AuditQueryRequestDto, ChatReferenceDto, ChatReferenceKindDto, CoreRequest, CoreResponse,
};
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

async fn member_in_project(
    team: &TeamStore,
    project: &ProjectId,
    name: &str,
    client_id: &str,
    role: ProjectRole,
) -> AuthenticatedPrincipal {
    let principal = human_token(team, name, client_id).await;
    let record = team
        .get_principal(principal.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(project, &record.id, role)
        .await
        .unwrap();
    principal
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
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
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

async fn ensure_default(daemon: &CoreDaemon, client: &str, project: &str) -> String {
    match call(
        daemon,
        client,
        CoreRequest::ChatChannelEnsure {
            project_id: project.to_owned(),
            name: None,
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
    idempotency_key: Option<&str>,
) -> codegg::protocol::core::ChatMessageDto {
    match call(
        daemon,
        client,
        CoreRequest::ChatSend {
            channel_id: channel.to_owned(),
            body: body.to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: idempotency_key.map(str::to_owned),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => message,
        other => panic!("expected chat message, got {other:?}"),
    }
}

// ── Capabilities and happy-path exchange ─────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_chat_capabilities_shape() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool), None, None);
    match call(&daemon, "local-daemon", CoreRequest::ChatCapabilities).await {
        CoreResponse::ChatCapabilities { capabilities } => {
            assert!(capabilities.supported);
            assert_eq!(capabilities.protocol_version, 1);
            assert!(capabilities.max_body_bytes >= 1024);
            assert!(capabilities.max_page_limit >= 10);
            assert!(capabilities.composing_ttl_secs >= 5);
            assert!(capabilities.retention_max_messages >= 5);
        }
        other => panic!("expected chat capabilities, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_member_can_ensure_send_history_sync() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);

    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
    let first = send(&daemon, "client-member", &channel, "hello team", None).await;
    assert_eq!(first.seq, 1);
    assert_eq!(first.body, "hello team");
    assert!(!first.redacted);
    let second = send(&daemon, "client-member", &channel, "second update", None).await;
    assert_eq!(second.seq, 2);

    // Bounded history returns both in order.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatHistory {
            messages,
            next_cursor,
            truncated,
            ..
        } => {
            assert_eq!(messages.len(), 2);
            assert_eq!(messages[0].seq, 1);
            assert_eq!(messages[1].seq, 2);
            assert_eq!(next_cursor, 3);
            assert!(!truncated);
        }
        other => panic!("expected chat history, got {other:?}"),
    }

    // Incremental sync from a cursor resumes without resync.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatSync {
            channel_id: channel.clone(),
            from_seq: 2,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatSync {
            messages,
            next_cursor,
            resync_required,
            ..
        } => {
            assert!(!resync_required);
            assert_eq!(messages.len(), 1);
            assert_eq!(messages[0].seq, 2);
            assert_eq!(next_cursor, 3);
        }
        other => panic!("expected chat sync, got {other:?}"),
    }

    // A cursor past the high-water mark returns an empty page, not a resync.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatSync {
            channel_id: channel.clone(),
            from_seq: 99,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatSync {
            messages,
            next_cursor,
            resync_required,
            ..
        } => {
            assert!(!resync_required);
            assert!(messages.is_empty());
            assert_eq!(next_cursor, 99);
        }
        other => panic!("expected empty chat sync, got {other:?}"),
    }
}

// ── Replies, edits, redactions, revision conflicts ───────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_reply_edit_redact_revision_flow() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let author = member_in_project(
        &team,
        &project,
        "Author",
        "client-author",
        ProjectRole::Contributor,
    )
    .await;
    let other = member_in_project(
        &team,
        &project,
        "Other",
        "client-other",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-author", author);
    register_client(&daemon, "client-other", other);

    let channel = ensure_default(&daemon, "client-author", project.as_str()).await;
    let parent = send(&daemon, "client-author", &channel, "parent question", None).await;

    // Reply with a mention and a typed reference.
    match call(
        &daemon,
        "client-other",
        CoreRequest::ChatSend {
            channel_id: channel.clone(),
            body: "reply with context".to_owned(),
            reply_to: Some(parent.message_id.clone()),
            thread_root: Some(parent.message_id.clone()),
            mentions: vec!["principal-other".to_owned()],
            references: vec![ChatReferenceDto {
                kind: ChatReferenceKindDto::Session,
                target_id: "session-1".to_owned(),
                display_hint: Some("design session".to_owned()),
            }],
            idempotency_key: None,
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => {
            assert_eq!(
                message.reply_to.as_deref(),
                Some(parent.message_id.as_str())
            );
            assert_eq!(message.references.len(), 1);
        }
        other => panic!("expected reply message, got {other:?}"),
    }

    // Non-authors cannot edit.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-other",
                CoreRequest::ChatEdit {
                    channel_id: channel.clone(),
                    message_id: parent.message_id.clone(),
                    expected_revision: 1,
                    new_body: "hijack".to_owned(),
                },
            )
            .await
        ),
        "chat_not_author"
    );

    // Stale revisions conflict with a typed error.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-author",
                CoreRequest::ChatEdit {
                    channel_id: channel.clone(),
                    message_id: parent.message_id.clone(),
                    expected_revision: 99,
                    new_body: "stale".to_owned(),
                },
            )
            .await
        ),
        "chat_revision_conflict"
    );

    // Author edits, then redacts; revisions advance 1 -> 2 -> 3.
    match call(
        &daemon,
        "client-author",
        CoreRequest::ChatEdit {
            channel_id: channel.clone(),
            message_id: parent.message_id.clone(),
            expected_revision: 1,
            new_body: "edited question".to_owned(),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => {
            assert_eq!(message.revision, 2);
            assert_eq!(message.body, "edited question");
        }
        other => panic!("expected edited message, got {other:?}"),
    }
    match call(
        &daemon,
        "client-author",
        CoreRequest::ChatRedact {
            channel_id: channel.clone(),
            message_id: parent.message_id.clone(),
            expected_revision: Some(2),
            reason: Some("cleanup".to_owned()),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => {
            assert_eq!(message.revision, 3);
            assert!(message.redacted);
            assert_eq!(message.body, "[REDACTED]");
        }
        other => panic!("expected redacted message, got {other:?}"),
    }
}

// ── Idempotent retries ───────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_idempotent_retry_returns_original_without_duplicate() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);

    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
    let first = send(
        &daemon,
        "client-member",
        &channel,
        "original send",
        Some("retry-key-1"),
    )
    .await;
    // A retry with a changed body converges on the original row.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatSend {
            channel_id: channel.clone(),
            body: "changed body on retry".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: Some("retry-key-1".to_owned()),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, duplicate } => {
            assert!(duplicate);
            assert_eq!(message.message_id, first.message_id);
            assert_eq!(message.seq, first.seq);
            assert_eq!(message.body, "original send");
        }
        other => panic!("expected duplicate message, got {other:?}"),
    }
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => assert_eq!(messages.len(), 1),
        other => panic!("expected single stored message, got {other:?}"),
    }
}

// ── Authorization and privacy ────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_unauthorized_callers_cannot_enumerate_or_read() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    let viewer = member_in_project(
        &team,
        &project,
        "Viewer",
        "client-viewer",
        ProjectRole::Viewer,
    )
    .await;
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-member", member);
    register_client(&daemon, "client-viewer", viewer);
    register_client(&daemon, "client-outsider", outsider);

    // A member seeds one channel so the project is non-empty for members.
    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
    send(&daemon, "client-member", &channel, "seed", None).await;

    // Outsiders (no membership) and viewers (no project.chat) observe the
    // same not-found shape as a genuinely absent project on every chat
    // surface: enumeration, content, references, markers, and sync.
    for client in ["client-outsider", "client-viewer"] {
        assert_eq!(
            error_code(
                &call(
                    &daemon,
                    client,
                    CoreRequest::ChatChannelList {
                        project_id: project.as_str().to_owned(),
                        limit: None,
                    },
                )
                .await
            ),
            "project_not_found",
            "channel list must not leak for {client}"
        );
        assert_eq!(
            error_code(
                &call(
                    &daemon,
                    client,
                    CoreRequest::ChatHistory {
                        channel_id: channel.clone(),
                        from_seq: None,
                        limit: Some(10),
                    },
                )
                .await
            ),
            "project_not_found",
            "history must not leak for {client}"
        );
        assert_eq!(
            error_code(
                &call(
                    &daemon,
                    client,
                    CoreRequest::ChatSend {
                        channel_id: channel.clone(),
                        body: "probe".to_owned(),
                        reply_to: None,
                        thread_root: None,
                        mentions: Vec::new(),
                        references: Vec::new(),
                        idempotency_key: None,
                    },
                )
                .await
            ),
            "project_not_found",
            "send must not leak for {client}"
        );
        assert_eq!(
            error_code(
                &call(
                    &daemon,
                    client,
                    CoreRequest::ChatSync {
                        channel_id: channel.clone(),
                        from_seq: 0,
                        limit: Some(10),
                    },
                )
                .await
            ),
            "project_not_found",
            "sync must not leak for {client}"
        );
        assert_eq!(
            error_code(
                &call(
                    &daemon,
                    client,
                    CoreRequest::ChatComposingList {
                        channel_id: channel.clone(),
                    },
                )
                .await
            ),
            "project_not_found",
            "composing must not leak for {client}"
        );
    }

    // Unknown channel ids are indistinguishable from denied ones for
    // every team principal (members included): the gate fails closed
    // before the handler can report a typed shape.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-outsider",
                CoreRequest::ChatHistory {
                    channel_id: "channel-absent".to_owned(),
                    from_seq: None,
                    limit: Some(10),
                },
            )
            .await
        ),
        "project_not_found"
    );
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-member",
                CoreRequest::ChatHistory {
                    channel_id: "channel-absent".to_owned(),
                    from_seq: None,
                    limit: Some(10),
                },
            )
            .await
        ),
        "project_not_found"
    );
    // The local owner passes the gate without a project binding, so the
    // handler reports the typed shape for genuinely unknown channels.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-local-unregistered",
                CoreRequest::ChatHistory {
                    channel_id: "channel-absent".to_owned(),
                    from_seq: None,
                    limit: Some(10),
                },
            )
            .await
        ),
        "chat_channel_not_found"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_channel_scope_isolation_across_projects() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    let member_a = member_in_project(
        &team,
        &project_a,
        "MemberA",
        "client-a",
        ProjectRole::Contributor,
    )
    .await;
    let member_b = member_in_project(
        &team,
        &project_b,
        "MemberB",
        "client-b",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-a", member_a);
    register_client(&daemon, "client-b", member_b);

    let channel_a = ensure_default(&daemon, "client-a", project_a.as_str()).await;
    let channel_b = ensure_default(&daemon, "client-b", project_b.as_str()).await;
    assert_ne!(channel_a, channel_b);
    send(&daemon, "client-a", &channel_a, "project a note", None).await;

    // Membership in A grants nothing in B.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-a",
                CoreRequest::ChatHistory {
                    channel_id: channel_b.clone(),
                    from_seq: None,
                    limit: Some(10),
                },
            )
            .await
        ),
        "project_not_found"
    );
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-b",
                CoreRequest::ChatHistory {
                    channel_id: channel_a.clone(),
                    from_seq: None,
                    limit: Some(10),
                },
            )
            .await
        ),
        "project_not_found"
    );
}

// ── Composing, read markers, restart ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_composing_and_read_markers_round_trip() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);

    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
    send(&daemon, "client-member", &channel, "m1", None).await;
    send(&daemon, "client-member", &channel, "m2", None).await;

    // Composing sets a content-free lease visible to project members.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatComposingSet {
            channel_id: channel.clone(),
            composing: true,
        },
    )
    .await
    {
        CoreResponse::ChatComposing { composing, .. } => {
            assert_eq!(composing.len(), 1);
            assert_eq!(composing[0].channel_id, channel);
        }
        other => panic!("expected composing snapshot, got {other:?}"),
    }
    // Clearing removes the lease without touching durable messages.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatComposingSet {
            channel_id: channel.clone(),
            composing: false,
        },
    )
    .await
    {
        CoreResponse::ChatComposing { composing, .. } => assert!(composing.is_empty()),
        other => panic!("expected empty composing snapshot, got {other:?}"),
    }

    // Read markers advance forward only.
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatReadSet {
            channel_id: channel.clone(),
            last_read_seq: 2,
        },
    )
    .await
    {
        CoreResponse::ChatReadMarker { last_read_seq, .. } => assert_eq!(last_read_seq, 2),
        other => panic!("expected read marker, got {other:?}"),
    }
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatReadGet {
            channel_id: channel.clone(),
        },
    )
    .await
    {
        CoreResponse::ChatReadMarker { last_read_seq, .. } => assert_eq!(last_read_seq, 2),
        other => panic!("expected stored read marker, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_restart_preserves_order_and_markers_drops_composing() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;

    let channel = {
        let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
        register_client(&daemon, "client-member", member);
        let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
        send(&daemon, "client-member", &channel, "restart-0", None).await;
        send(&daemon, "client-member", &channel, "restart-1", None).await;
        send(&daemon, "client-member", &channel, "restart-2", None).await;
        match call(
            &daemon,
            "client-member",
            CoreRequest::ChatReadSet {
                channel_id: channel.clone(),
                last_read_seq: 2,
            },
        )
        .await
        {
            CoreResponse::ChatReadMarker { .. } => {}
            other => panic!("expected read marker, got {other:?}"),
        }
        call(
            &daemon,
            "client-member",
            CoreRequest::ChatComposingSet {
                channel_id: channel.clone(),
                composing: true,
            },
        )
        .await;
        channel
    };

    // A fresh daemon over the same pool observes durable order and
    // markers, but composing leases do not survive restart.
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let member = member_in_project(
        &team,
        &project,
        "MemberAfter",
        "client-after",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-after", member);
    match call(
        &daemon,
        "client-after",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => {
            assert_eq!(messages.len(), 3);
            assert_eq!(messages[0].seq, 1);
            assert_eq!(messages[2].seq, 3);
        }
        other => panic!("expected restarted history, got {other:?}"),
    }
    match call(
        &daemon,
        "client-after",
        CoreRequest::ChatComposingList {
            channel_id: channel.clone(),
        },
    )
    .await
    {
        CoreResponse::ChatComposing { composing, .. } => assert!(composing.is_empty()),
        other => panic!("expected dropped composing, got {other:?}"),
    }
}

// ── Concurrency, bounds, secrets, audit separation ───────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_concurrent_sends_converge_on_distinct_sequences() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);
    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;

    let (a, b, c, d) = tokio::join!(
        call(
            &daemon,
            "client-member",
            CoreRequest::ChatSend {
                channel_id: channel.clone(),
                body: "race-a".to_owned(),
                reply_to: None,
                thread_root: None,
                mentions: Vec::new(),
                references: Vec::new(),
                idempotency_key: Some("race-a".to_owned()),
            },
        ),
        call(
            &daemon,
            "client-member",
            CoreRequest::ChatSend {
                channel_id: channel.clone(),
                body: "race-b".to_owned(),
                reply_to: None,
                thread_root: None,
                mentions: Vec::new(),
                references: Vec::new(),
                idempotency_key: Some("race-b".to_owned()),
            },
        ),
        call(
            &daemon,
            "client-member",
            CoreRequest::ChatSend {
                channel_id: channel.clone(),
                body: "race-c".to_owned(),
                reply_to: None,
                thread_root: None,
                mentions: Vec::new(),
                references: Vec::new(),
                idempotency_key: Some("race-c".to_owned()),
            },
        ),
        call(
            &daemon,
            "client-member",
            CoreRequest::ChatSend {
                channel_id: channel.clone(),
                body: "race-d".to_owned(),
                reply_to: None,
                thread_root: None,
                mentions: Vec::new(),
                references: Vec::new(),
                idempotency_key: Some("race-d".to_owned()),
            },
        ),
    );
    let mut seqs = Vec::new();
    for response in [a, b, c, d] {
        match response {
            CoreResponse::ChatMessage { message, .. } => seqs.push(message.seq),
            other => panic!("expected raced message, got {other:?}"),
        }
    }
    seqs.sort_unstable();
    assert_eq!(seqs, vec![1, 2, 3, 4]);
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_body_bounds_and_secret_redaction() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);
    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;

    // Empty and oversized bodies are rejected with typed errors.
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-member",
                CoreRequest::ChatSend {
                    channel_id: channel.clone(),
                    body: String::new(),
                    reply_to: None,
                    thread_root: None,
                    mentions: Vec::new(),
                    references: Vec::new(),
                    idempotency_key: None,
                },
            )
            .await
        ),
        "chat_invalid_input"
    );
    assert_eq!(
        error_code(
            &call(
                &daemon,
                "client-member",
                CoreRequest::ChatSend {
                    channel_id: channel.clone(),
                    body: "x".repeat(9 * 1024),
                    reply_to: None,
                    thread_root: None,
                    mentions: Vec::new(),
                    references: Vec::new(),
                    idempotency_key: None,
                },
            )
            .await
        ),
        "chat_body_too_large"
    );

    // Credential-like values persist redacted, never in cleartext.
    let stored = send(
        &daemon,
        "client-member",
        &channel,
        "deploy with api_key=hunter2-green today",
        None,
    )
    .await;
    assert!(stored.body.contains("[REDACTED]"));
    assert!(!stored.body.contains("hunter2-green"));
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_free_text_has_no_execution_semantics() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);
    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;

    // Command-like, mention-like, and action-like text persists as inert
    // text: every body round-trips verbatim through history and the
    // channel holds exactly the six sent messages with no side effects.
    for body in [
        "/observe session-1",
        "!rm -rf /tmp/work",
        "!!cat /etc/passwd",
        "@owner please run the tests",
        "run: cargo test --workspace",
        "agent: invoke review now",
    ] {
        let stored = send(&daemon, "client-member", &channel, body, None).await;
        assert_eq!(stored.body, body);
    }
    match call(
        &daemon,
        "client-member",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => {
            assert_eq!(messages.len(), 6);
            assert_eq!(messages[0].body, "/observe session-1");
            assert_eq!(messages[5].body, "agent: invoke review now");
        }
        other => panic!("expected inert chat history, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_chat_storage_stays_separate_from_audit() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let member = member_in_project(
        &team,
        &project,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maintainer",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-member", member);
    register_client(&daemon, "client-maintainer", maintainer);

    let channel = ensure_default(&daemon, "client-member", project.as_str()).await;
    send(
        &daemon,
        "client-member",
        &channel,
        "purple-elephant-chat-body with api_key=hunter2-violet",
        None,
    )
    .await;

    // A maintainer holding audit.read observes no chat body material in
    // the structural audit page: chat storage remains separate.
    match call(
        &daemon,
        "client-maintainer",
        CoreRequest::AuditQuery {
            query: AuditQueryRequestDto {
                project_id: project.as_str().to_owned(),
                action_filter: None,
                principal_filter: None,
                from_seq: None,
                limit: Some(100),
            },
        },
    )
    .await
    {
        CoreResponse::AuditPage { events, .. } => {
            let serialized = serde_json::to_string(&events).expect("serialize audit page");
            assert!(
                !serialized.contains("purple-elephant-chat-body"),
                "chat body must not enter the audit store"
            );
            assert!(
                !serialized.contains("hunter2-violet"),
                "chat secrets must not enter the audit store"
            );
        }
        other => panic!("expected audit page, got {other:?}"),
    }
}
