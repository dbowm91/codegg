//! Team Collaboration Corrective M002 — project/channel chat access policy.
//!
//! Boundary proof for ADR-0006: role defaults stay the compatibility
//! baseline (Viewer denied, Contributor+ allowed); a revisioned
//! daemon-owned overlay adds project principal overrides plus
//! per-channel mode (`inherit_project` | `restricted`) and channel
//! principal overrides. Active membership is always required first; a
//! chat grant never implies execution authority.
//!
//! - Viewer default deny; Contributor default allow (old-client compat).
//! - Viewer project allow; Viewer one-channel allow.
//! - Contributor project deny; channel deny over project allow.
//! - Restricted channel explicit allowlist + list filtering.
//! - Revoked member with stale allow denied immediately.
//! - Direct denied-channel lookup indistinguishable from absent.
//! - Concurrent override CAS converges; stale revisions conflict.
//! - Restart preserves policy; reconnect re-evaluates without reconnect.
//! - Structured actions still require their semantic capability.
//! - Cross-project locator probes fail closed.
//! - Policy administration requires `member.manage` (Owner only).

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    ChatChannelModeDto, ChatPolicyDecisionDto, CoreRequest, CoreResponse,
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

async fn call_code(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> String {
    let resp = call(daemon, client, request).await;
    error_code(&resp)
}

fn error_code(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code.clone(),
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

async fn ensure_named(daemon: &CoreDaemon, client: &str, project: &str, name: &str) -> String {
    match call(
        daemon,
        client,
        CoreRequest::ChatChannelEnsure {
            project_id: project.to_owned(),
            name: Some(name.to_owned()),
        },
    )
    .await
    {
        CoreResponse::ChatChannel { channel } => channel.channel_id,
        other => panic!("expected named chat channel, got {other:?}"),
    }
}

async fn send_ok(daemon: &CoreDaemon, client: &str, channel: &str, body: &str) {
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
            idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { .. } => {}
        other => panic!("expected chat message, got {other:?}"),
    }
}

fn project_policy_revision(response: &CoreResponse) -> u64 {
    match response {
        CoreResponse::ChatPolicy { project, .. } => project.revision,
        other => panic!("expected chat policy, got {other:?}"),
    }
}

// ── Role-default compatibility (old-client behavior) ─────────────────

#[tokio::test(flavor = "current_thread")]
async fn viewer_default_deny_contributor_default_allow() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    let contributor = member_in_project(
        &team,
        &project,
        "Contrib",
        "c-contrib",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "c-owner", owner);
    register_client(&daemon, "c-viewer", viewer);
    register_client(&daemon, "c-contrib", contributor);

    // Owner creates the default channel under project-level access.
    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;

    // Viewer with no policy rows is denied at every entry point.
    for request in [
        CoreRequest::ChatChannelList {
            project_id: project.as_str().to_owned(),
            limit: None,
        },
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: None,
        },
        CoreRequest::ChatSync {
            channel_id: channel.clone(),
            from_seq: 0,
            limit: None,
        },
        CoreRequest::ChatComposingList {
            channel_id: channel.clone(),
        },
        CoreRequest::ChatReadGet {
            channel_id: channel.clone(),
        },
    ] {
        let resp = call(&daemon, "c-viewer", request).await;
        let code = error_code(&resp);
        assert_eq!(
            code.as_str(),
            "project_not_found",
            "viewer default must deny"
        );
    }
    // Viewer send is privacy-safe not-found as well.
    let code = call_code(
        &daemon,
        "c-viewer",
        CoreRequest::ChatSend {
            channel_id: channel.clone(),
            body: "hello".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "viewer send denied, got {code}"
    );

    // Contributor with no policy rows retains the compatible default.
    send_ok(&daemon, "c-contrib", &channel, "contributor hello").await;
    match call(
        &daemon,
        "c-contrib",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => assert!(!messages.is_empty()),
        other => panic!("contributor history allowed, got {other:?}"),
    }
}

// ── Viewer project grant ─────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn viewer_project_allow_grants_chat_without_write_authority() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;

    // Owner grants the viewer project chat.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => {
            assert_eq!(project.revision, 1);
            assert_eq!(project.overrides.len(), 1);
        }
        other => panic!("expected policy grant, got {other:?}"),
    }

    // Viewer can now observe session state (read-only) and participate
    // in chat, without receiving write authority.
    send_ok(&daemon, "c-viewer", &channel, "viewer question").await;
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => assert_eq!(messages.len(), 1),
        other => panic!("viewer history allowed after grant, got {other:?}"),
    }
    // Structured action still requires its semantic capability: a viewer
    // chat grant does not confer `job.submit` or `agent.delegate`.
    let code = call_code(
        &daemon,
        "c-viewer",
        CoreRequest::ChatActionSubmit {
            channel_id: channel.clone(),
            message_id: "msg-does-not-matter".to_owned(),
            action: codegg::protocol::core::ChatActionSubmitDto::JobReference {
                job_id: "job-1".to_owned(),
                title: None,
            },
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        },
    )
    .await;
    assert_ne!(code.as_str(), "unimplemented");
    // Either a message-linkage failure or a capability denial proves no
    // execution happened; the key assertion is no job/action row exists
    // (covered in the dedicated non-escalation test below).
}

// ── Viewer one-channel grant ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn viewer_one_channel_allow_scopes_to_single_channel() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    let general = ensure_default(&daemon, "c-owner", project.as_str()).await;
    let side = ensure_named(&daemon, "c-owner", project.as_str(), "side").await;

    // Owner grants the viewer one channel only.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatChannelPolicySet {
            channel_id: side.clone(),
            mode: None,
            principal_id: Some(viewer_id.as_str().to_owned()),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { channel, .. } => assert!(channel.is_some()),
        other => panic!("expected channel grant, got {other:?}"),
    }

    // Viewer can send in the granted channel but not the default.
    send_ok(&daemon, "c-viewer", &side, "viewer in side").await;
    let code = call_code(
        &daemon,
        "c-viewer",
        CoreRequest::ChatSend {
            channel_id: general.clone(),
            body: "viewer in general".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "viewer denied in non-granted channel, got {code}"
    );

    // Channel listing exposes exactly the granted channel.
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatChannelList {
            project_id: project.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatChannelList { channels, .. } => {
            assert_eq!(channels.len(), 1);
            assert_eq!(channels[0].channel_id, side);
        }
        other => panic!("expected filtered list, got {other:?}"),
    }
}

// ── Contributor deny ─────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn contributor_project_deny_blocks_chat_without_losing_other_caps() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let contrib = member_in_project(
        &team,
        &project,
        "Contrib",
        "c-contrib",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-contrib", contrib.clone());
    let contrib_id = contrib.principal_id().clone();

    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;
    send_ok(&daemon, "c-contrib", &channel, "before deny").await;

    // Owner denies the contributor project chat.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: contrib_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { .. } => {}
        other => panic!("expected deny, got {other:?}"),
    }

    // Chat is now denied on every entry point.
    for request in [
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: None,
        },
        CoreRequest::ChatSync {
            channel_id: channel.clone(),
            from_seq: 0,
            limit: None,
        },
        CoreRequest::ChatReadGet {
            channel_id: channel.clone(),
        },
    ] {
        let code = call_code(&daemon, "c-contrib", request).await;
        assert!(
            code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
            "denied contributor, got {code}"
        );
    }

    // Unrelated capabilities are untouched: the contributor still holds
    // the role's write bundle (checked directly against team state).
    let membership = team
        .get_membership(&project, &contrib_id)
        .await
        .unwrap()
        .unwrap();
    assert!(membership
        .effective_capabilities()
        .has(codegg_core::team::Capability::FileModify));
    assert!(membership
        .effective_capabilities()
        .has(codegg_core::team::Capability::SessionCreate));
}

// ── Channel deny over project allow + restricted allowlist ──────────

#[tokio::test(flavor = "current_thread")]
async fn channel_deny_overrides_project_allow_and_restricted_allowlists() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    let contrib = member_in_project(
        &team,
        &project,
        "Contrib",
        "c-contrib",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    register_client(&daemon, "c-contrib", contrib.clone());
    let viewer_id = viewer.principal_id().clone();
    let contrib_id = contrib.principal_id().clone();

    let general = ensure_default(&daemon, "c-owner", project.as_str()).await;
    let private = ensure_named(&daemon, "c-owner", project.as_str(), "private").await;

    // Viewer gets project chat, then is denied one channel.
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await;
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatChannelPolicySet {
            channel_id: private.clone(),
            mode: None,
            principal_id: Some(viewer_id.as_str().to_owned()),
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: None,
        },
    )
    .await;
    send_ok(&daemon, "c-viewer", &general, "viewer in general").await;
    let code = call_code(
        &daemon,
        "c-viewer",
        CoreRequest::ChatHistory {
            channel_id: private.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "channel deny wins, got {code}"
    );

    // Restrict the private channel: contributor default flips to deny.
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatChannelPolicySet {
            channel_id: private.clone(),
            mode: Some(ChatChannelModeDto::Restricted),
            principal_id: None,
            decision: None,
            expected_revision: None,
        },
    )
    .await;
    // Contributor (no explicit override) is now denied in the
    // restricted channel but still allowed in general.
    send_ok(&daemon, "c-contrib", &general, "contrib in general").await;
    let code = call_code(
        &daemon,
        "c-contrib",
        CoreRequest::ChatHistory {
            channel_id: private.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "restricted default denies, got {code}"
    );
    // Explicit allowlist restores the contributor in the restricted
    // channel.
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatChannelPolicySet {
            channel_id: private.clone(),
            mode: None,
            principal_id: Some(contrib_id.as_str().to_owned()),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await;
    send_ok(&daemon, "c-contrib", &private, "contrib allowlisted").await;

    // Restricted channels are not enumerable to denied members: the
    // viewer (denied in private) lists only general.
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatChannelList {
            project_id: project.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatChannelList { channels, .. } => {
            assert!(channels.iter().any(|c| c.channel_id == general));
            assert!(!channels.iter().any(|c| c.channel_id == private));
        }
        other => panic!("expected filtered list, got {other:?}"),
    }
}

// ── Revocation wins over stale allow ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn revoked_member_with_stale_allow_is_denied_immediately() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_record = team
        .get_principal(viewer.principal_id())
        .await
        .unwrap()
        .unwrap();

    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_record.id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await;
    send_ok(&daemon, "c-viewer", &channel, "before revoke").await;

    // Revoke the membership; the stale allow row must not resurrect it.
    let membership = team
        .get_membership(&project, &viewer_record.id)
        .await
        .unwrap()
        .unwrap();
    team.revoke_membership(&project, &viewer_record.id, membership.revision)
        .await
        .unwrap();

    // Reconnect is irrelevant: revocation applies at request time.
    let code = call_code(
        &daemon,
        "c-viewer",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    assert_eq!(code.as_str(), "project_not_found");
}

// ── Direct denied-channel lookup + cross-project probe ───────────────

#[tokio::test(flavor = "current_thread")]
async fn denied_channel_lookup_and_cross_project_probe_are_indistinguishable() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    let owner_a =
        member_in_project(&team, &project_a, "OwnerA", "c-owner-a", ProjectRole::Owner).await;
    let viewer_a = member_in_project(
        &team,
        &project_a,
        "ViewerA",
        "c-viewer-a",
        ProjectRole::Viewer,
    )
    .await;
    let owner_b =
        member_in_project(&team, &project_b, "OwnerB", "c-owner-b", ProjectRole::Owner).await;
    register_client(&daemon, "c-owner-a", owner_a);
    register_client(&daemon, "c-viewer-a", viewer_a);
    register_client(&daemon, "c-owner-b", owner_b);

    let channel_a = ensure_default(&daemon, "c-owner-a", project_a.as_str()).await;
    let channel_b = ensure_default(&daemon, "c-owner-b", project_b.as_str()).await;

    // Denied channel in the caller's own project.
    let denied_code = call_code(
        &daemon,
        "c-viewer-a",
        CoreRequest::ChatHistory {
            channel_id: channel_a.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    // Cross-project probe with a real channel id from another project.
    let probe_code = call_code(
        &daemon,
        "c-viewer-a",
        CoreRequest::ChatHistory {
            channel_id: channel_b.clone(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    // Genuinely unknown channel id.
    let unknown_code = call_code(
        &daemon,
        "c-viewer-a",
        CoreRequest::ChatHistory {
            channel_id: "chan-doesnotexist01".to_owned(),
            from_seq: None,
            limit: None,
        },
    )
    .await;
    for code in [&denied_code, &probe_code, &unknown_code] {
        let code: &str = code;
        assert!(
            code == "project_not_found" || code == "chat_channel_not_found",
            "privacy-safe not-found, got {code}"
        );
    }
    // Denied and probe shapes match: no existence oracle.
    assert_eq!(denied_code, probe_code);
}

// ── Policy administration authorization matrix ───────────────────────

#[tokio::test(flavor = "current_thread")]
async fn policy_admin_requires_member_manage() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let maintainer =
        member_in_project(&team, &project, "Maint", "c-maint", ProjectRole::Maintainer).await;
    let contrib = member_in_project(
        &team,
        &project,
        "Contrib",
        "c-contrib",
        ProjectRole::Contributor,
    )
    .await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-maint", maintainer);
    register_client(&daemon, "c-contrib", contrib);
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    // Non-owners cannot inspect or mutate policy.
    for (client, request) in [
        (
            "c-viewer",
            CoreRequest::ChatPolicyGet {
                project_id: project.as_str().to_owned(),
                channel_id: None,
            },
        ),
        (
            "c-contrib",
            CoreRequest::ChatPolicyList {
                project_id: project.as_str().to_owned(),
            },
        ),
        (
            "c-maint",
            CoreRequest::ChatProjectPolicySet {
                project_id: project.as_str().to_owned(),
                principal_id: viewer_id.as_str().to_owned(),
                decision: Some(ChatPolicyDecisionDto::Allow),
                expected_revision: None,
            },
        ),
    ] {
        let code = call_code(&daemon, client, request).await;
        assert_eq!(
            code.as_str(),
            "project_not_found",
            "non-owner denied: {client}"
        );
    }

    // Owner can inspect (empty) and mutate.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatPolicyGet {
            project_id: project.as_str().to_owned(),
            channel_id: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, channel } => {
            assert_eq!(project.revision, 0);
            assert!(project.overrides.is_empty());
            assert!(channel.is_none());
        }
        other => panic!("owner get allowed, got {other:?}"),
    }
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatPolicyList {
            project_id: project.as_str().to_owned(),
        },
    )
    .await
    {
        CoreResponse::ChatPolicyList { channels, .. } => assert!(channels.is_empty()),
        other => panic!("owner list allowed, got {other:?}"),
    }
}

// ── Revision CAS, idempotency, restart ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn concurrent_override_cas_conflicts_and_duplicates_converge() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    // First grant: revision 0 -> 1.
    let first = call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: Some(0),
        },
    )
    .await;
    assert_eq!(project_policy_revision(&first), 1);

    // Duplicate identical set converges on the same revision.
    let dup = call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: Some(1),
        },
    )
    .await;
    assert_eq!(project_policy_revision(&dup), 1);

    // Stale revision fails with zero overwrite.
    let code = call_code(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Deny),
            expected_revision: Some(0),
        },
    )
    .await;
    assert_eq!(code.as_str(), "chat_revision_conflict");
    // The stored decision is still Allow.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatPolicyGet {
            project_id: project.as_str().to_owned(),
            channel_id: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => {
            assert_eq!(project.revision, 1);
            assert_eq!(project.overrides.len(), 1);
        }
        other => panic!("expected policy, got {other:?}"),
    }

    // Clear converges; clearing an absent row converges too.
    let cleared = call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: None,
            expected_revision: Some(1),
        },
    )
    .await;
    assert_eq!(project_policy_revision(&cleared), 2);
    let cleared_again = call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: None,
            expected_revision: Some(2),
        },
    )
    .await;
    assert_eq!(project_policy_revision(&cleared_again), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_preserves_policy_and_reconnect_reevaluates() {
    let pool = test_pool().await;
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    let viewer_id = viewer.principal_id().clone();

    {
        let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
        register_client(&daemon, "c-owner", owner.clone());
        register_client(&daemon, "c-viewer", viewer.clone());
        let _channel = ensure_default(&daemon, "c-owner", project.as_str()).await;
        call(
            &daemon,
            "c-owner",
            CoreRequest::ChatProjectPolicySet {
                project_id: project.as_str().to_owned(),
                principal_id: viewer_id.as_str().to_owned(),
                decision: Some(ChatPolicyDecisionDto::Allow),
                expected_revision: None,
            },
        )
        .await;
    }
    // Restart on the same pool: fresh daemon, same durable rows.
    let daemon2 = CoreDaemon::new(Some(pool.clone()), None, None);
    register_client(&daemon2, "c-owner", owner.clone());
    register_client(&daemon2, "c-viewer", viewer.clone());
    match call(
        &daemon2,
        "c-owner",
        CoreRequest::ChatPolicyGet {
            project_id: project.as_str().to_owned(),
            channel_id: None,
        },
    )
    .await
    {
        CoreResponse::ChatPolicy { project, .. } => assert_eq!(project.revision, 1),
        other => panic!("policy survives restart, got {other:?}"),
    }
    // Reconnect re-evaluates: revoke the grant and the same (unreconnected)
    // viewer client is denied on its next request.
    call(
        &daemon2,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: None,
            expected_revision: None,
        },
    )
    .await;
    match call(
        &daemon2,
        "c-viewer",
        CoreRequest::ChatChannelList {
            project_id: project.as_str().to_owned(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatChannelList { channels, .. } => assert!(channels.is_empty()),
        CoreResponse::Error { code, .. } => assert_eq!(code.as_str(), "project_not_found"),
        other => panic!("viewer denied after revoke, got {other:?}"),
    }
}

// ── Structured-action non-escalation ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn structured_action_denied_despite_chat_allow_without_semantic_cap() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await;
    // Viewer sends a real message so the action has a valid linkage.
    let message_id = match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatSend {
            channel_id: channel.clone(),
            body: "task context".to_owned(),
            reply_to: None,
            thread_root: None,
            mentions: Vec::new(),
            references: Vec::new(),
            idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => message.message_id,
        other => panic!("owner send, got {other:?}"),
    };
    // Viewer holds chat access but lacks `agent.delegate`: the
    // agent-task action must deny and create no action row.
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatActionSubmit {
            channel_id: channel.clone(),
            message_id: message_id.clone(),
            action: codegg::protocol::core::ChatActionSubmitDto::AgentTask {
                workspace_id: "ws".to_owned(),
                agent: "agent".to_owned(),
                prompt: "do work".to_owned(),
                session_id: None,
                title: Some("task".to_owned()),
            },
            idempotency_key: uuid::Uuid::new_v4().to_string(),
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => assert_ne!(code, "unimplemented"),
        other => panic!("viewer agent-task denied, got {other:?}"),
    }
    // No action row exists for the denied attempt.
    match call(
        &daemon,
        "c-owner",
        CoreRequest::ChatActionList {
            channel_id: channel.clone(),
            message_id: Some(message_id),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatActionList { actions, .. } => assert!(actions.is_empty()),
        other => panic!("no action created, got {other:?}"),
    }
}

// ── Read-marker and composing follow the same policy ─────────────────

#[tokio::test(flavor = "current_thread")]
async fn read_markers_and_composing_follow_effective_policy() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let owner = member_in_project(&team, &project, "Owner", "c-owner", ProjectRole::Owner).await;
    let viewer =
        member_in_project(&team, &project, "Viewer", "c-viewer", ProjectRole::Viewer).await;
    register_client(&daemon, "c-owner", owner.clone());
    register_client(&daemon, "c-viewer", viewer.clone());
    let viewer_id = viewer.principal_id().clone();

    let channel = ensure_default(&daemon, "c-owner", project.as_str()).await;

    // Denied viewer cannot touch read markers or composing.
    for request in [
        CoreRequest::ChatReadSet {
            channel_id: channel.clone(),
            last_read_seq: 1,
        },
        CoreRequest::ChatComposingSet {
            channel_id: channel.clone(),
            composing: true,
        },
    ] {
        let resp = call(&daemon, "c-viewer", request).await;
        let code = error_code(&resp);
        assert!(
            code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
            "denied viewer, got {code}"
        );
    }
    // Grant project chat: markers and composing now round-trip.
    call(
        &daemon,
        "c-owner",
        CoreRequest::ChatProjectPolicySet {
            project_id: project.as_str().to_owned(),
            principal_id: viewer_id.as_str().to_owned(),
            decision: Some(ChatPolicyDecisionDto::Allow),
            expected_revision: None,
        },
    )
    .await;
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatReadSet {
            channel_id: channel.clone(),
            last_read_seq: 1,
        },
    )
    .await
    {
        CoreResponse::ChatReadMarker { last_read_seq, .. } => assert_eq!(last_read_seq, 1),
        other => panic!("marker allowed after grant, got {other:?}"),
    }
    match call(
        &daemon,
        "c-viewer",
        CoreRequest::ChatComposingSet {
            channel_id: channel.clone(),
            composing: true,
        },
    )
    .await
    {
        CoreResponse::ChatComposing { .. } => {}
        other => panic!("composing allowed after grant, got {other:?}"),
    }
}

// ── Foreign channel policy administration fails closed ───────────────

#[tokio::test(flavor = "current_thread")]
async fn foreign_channel_policy_probe_fails_closed() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    let owner_a =
        member_in_project(&team, &project_a, "OwnerA", "c-owner-a", ProjectRole::Owner).await;
    let owner_b =
        member_in_project(&team, &project_b, "OwnerB", "c-owner-b", ProjectRole::Owner).await;
    register_client(&daemon, "c-owner-a", owner_a.clone());
    register_client(&daemon, "c-owner-b", owner_b.clone());

    let channel_b = ensure_default(&daemon, "c-owner-b", project_b.as_str()).await;

    // Owner of A probes B's channel id through A's admin path is
    // impossible (channel resolves to B, gate denies); probing the
    // channel id directly also fails closed.
    let code = call_code(
        &daemon,
        "c-owner-a",
        CoreRequest::ChatChannelPolicySet {
            channel_id: channel_b.clone(),
            mode: Some(ChatChannelModeDto::Restricted),
            principal_id: None,
            decision: None,
            expected_revision: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "foreign channel fails closed, got {code}"
    );
    // Unknown channel id fails the same way.
    let code = call_code(
        &daemon,
        "c-owner-a",
        CoreRequest::ChatChannelPolicySet {
            channel_id: "chan-unknown00001".to_owned(),
            mode: Some(ChatChannelModeDto::Restricted),
            principal_id: None,
            decision: None,
            expected_revision: None,
        },
    )
    .await;
    assert!(
        code.as_str() == "project_not_found" || code.as_str() == "chat_channel_not_found",
        "unknown channel fails closed, got {code}"
    );
}
