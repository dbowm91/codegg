//! Project Collaboration M003 — separately authorized structured chat actions.
//!
//! Boundary proof for the plan: explicit typed message-associated actions
//! for agent tasks/review requests/job submits/references through the
//! existing daemon authorization, durable job, and audit owners, while
//! free-text messages have no privileged execution semantics.
//!
//! - Free-text corpus (command-like bodies) creates no job and no action.
//! - Authorized/denied matrix: Contributor submits jobs, Maintainer submits
//!   agent tasks/reviews, Viewer is denied at the gate, Contributor is
//!   denied for agent tasks (lacks `agent.delegate`).
//! - Message/project mismatch fails closed without execution.
//! - Duplicate retransmission converges on one action and one job.
//! - Restart preserves idempotency across a fresh daemon on the same pool.
//! - Membership revocation denies further actions.
//! - Task/job cancellation follows canonical job semantics; chat history
//!   stays intact.
//! - Audit links message -> decision -> action -> job with structural
//!   locators only (no prompts/secrets).
//! - Secrets in action payloads are rejected; references fail closed.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{
    AuditQueryRequestDto, ChatActionSubmitDto, CoreRequest, CoreResponse,
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
            idempotency_key: Some(uuid::Uuid::new_v4().to_string()),
        },
    )
    .await
    {
        CoreResponse::ChatMessage { message, .. } => message,
        other => panic!("expected chat message, got {other:?}"),
    }
}

async fn workspace_for(daemon: &CoreDaemon) -> (tempfile::TempDir, String) {
    let tmp = tempfile::tempdir().expect("temp workspace");
    let record = daemon
        .workspaces
        .get_or_register(tmp.path())
        .await
        .expect("register workspace");
    let id = record.id.to_string();
    (tmp, id)
}

fn test_job_spec(workspace_id: &str) -> codegg::protocol::dto::JobSubmitDto {
    let payload = serde_json::to_value(codegg_core::jobs::JobPayload::Test {
        command: "echo ok".to_owned(),
        argv: vec!["echo".to_owned(), "ok".to_owned()],
        cwd: Some("/tmp".to_owned()),
        scope: None,
        parent_run_id: None,
    })
    .expect("test payload");
    codegg::protocol::dto::JobSubmitDto {
        submission_key: None,
        workspace_id: workspace_id.to_owned(),
        session_id: None,
        turn_id: None,
        kind: "test".to_owned(),
        priority: "interactive".to_owned(),
        source: serde_json::json!({"kind": "interactive"}),
        payload,
        timeout_ms: None,
        retry_max_attempts: 1,
        retryable_failures: Vec::new(),
        idempotency: "safe_repeat".to_owned(),
        not_before_ms: None,
        deadline_ms: None,
        schedule_id: None,
        depends_on: Vec::new(),
        labels: std::collections::HashMap::new(),
    }
}

async fn submit_action(
    daemon: &CoreDaemon,
    client: &str,
    channel: &str,
    message: &str,
    action: ChatActionSubmitDto,
    key: &str,
) -> CoreResponse {
    call(
        daemon,
        client,
        CoreRequest::ChatActionSubmit {
            channel_id: channel.to_owned(),
            message_id: message.to_owned(),
            action,
            idempotency_key: key.to_owned(),
        },
    )
    .await
}

fn action_job_id(response: &CoreResponse) -> (String, String, bool) {
    match response {
        CoreResponse::ChatAction { action, duplicate } => (
            action.action_id.clone(),
            action.job_id.clone().expect("action carries job"),
            *duplicate,
        ),
        other => panic!("expected chat action, got {other:?}"),
    }
}

async fn job_count(daemon: &CoreDaemon) -> usize {
    daemon
        .deps
        .job_store
        .list_jobs(codegg_core::jobs::store::JobStoreQuery::default())
        .await
        .expect("list jobs")
        .len()
}

async fn action_count(daemon: &CoreDaemon, client: &str, channel: &str) -> usize {
    match call(
        daemon,
        client,
        CoreRequest::ChatActionList {
            channel_id: channel.to_owned(),
            message_id: None,
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatActionList { actions, .. } => actions.len(),
        other => panic!("expected action list, got {other:?}"),
    }
}

// ── Free-text corpus: zero execution ─────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn free_text_corpus_creates_no_job_and_no_action() {
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

    let corpus = [
        "/chat-action-task msg agent do work",
        "!rm -rf /",
        "!!echo pwned",
        "@agent please run the tests",
        "```sh\ncargo test\n```",
        "job_submit {\"kind\": \"test\"}",
    ];
    for body in corpus {
        let stored = send(&daemon, "client-member", &channel, body).await;
        assert_eq!(stored.body, body, "free text must round-trip verbatim");
    }
    assert_eq!(
        job_count(&daemon).await,
        0,
        "free text must not create jobs"
    );
    assert_eq!(
        action_count(&daemon, "client-member", &channel).await,
        0,
        "free text must not create actions"
    );
    // History is exact and inert.
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
            assert_eq!(messages.len(), corpus.len());
            for (stored, expected) in messages.iter().zip(corpus.iter()) {
                assert_eq!(&stored.body, expected);
            }
        }
        other => panic!("expected history, got {other:?}"),
    }
}

// ── Authorized/denied matrix ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn authorized_denied_action_matrix() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let contributor = member_in_project(
        &team,
        &project,
        "Contributor",
        "client-contrib",
        ProjectRole::Contributor,
    )
    .await;
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
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
    register_client(&daemon, "client-contrib", contributor);
    register_client(&daemon, "client-maint", maintainer);
    register_client(&daemon, "client-viewer", viewer);

    let channel = ensure_default(&daemon, "client-contrib", project.as_str()).await;
    let message = send(&daemon, "client-contrib", &channel, "please review this").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    // Contributor (JobSubmit, no AgentDelegate) submits a generic test job.
    let spec = test_job_spec(&ws);
    let response = submit_action(
        &daemon,
        "client-contrib",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobSubmit {
            spec: Box::new(spec),
            title: Some("run tests".to_owned()),
        },
        "matrix-job-submit-1",
    )
    .await;
    let (_, job_id, duplicate) = action_job_id(&response);
    assert!(!duplicate);
    assert!(!job_id.is_empty());
    assert_eq!(job_count(&daemon).await, 1);

    // Contributor lacks agent.delegate: agent_task is denied, creates nothing.
    let before_jobs = job_count(&daemon).await;
    let before_actions = action_count(&daemon, "client-contrib", &channel).await;
    let denied = submit_action(
        &daemon,
        "client-contrib",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "review this change".to_owned(),
            session_id: None,
            title: None,
        },
        "matrix-agent-denied-1",
    )
    .await;
    assert_eq!(error_code(&denied), "chat_action_denied");
    assert_eq!(job_count(&daemon).await, before_jobs);
    assert_eq!(
        action_count(&daemon, "client-contrib", &channel).await,
        before_actions
    );

    // Maintainer (AgentDelegate) submits an agent task.
    let response = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "review this change carefully".to_owned(),
            session_id: None,
            title: Some("review request".to_owned()),
        },
        "matrix-agent-allow-1",
    )
    .await;
    let (_, _, duplicate) = action_job_id(&response);
    assert!(!duplicate);

    // Maintainer submits a review request (distinct kind, same service).
    let response = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::ReviewRequest {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "second review pass".to_owned(),
            session_id: None,
            title: None,
        },
        "matrix-review-allow-1",
    )
    .await;
    let (_, _, duplicate) = action_job_id(&response);
    assert!(!duplicate);

    // Viewer lacks project.chat: denied at the gate, creates nothing.
    let before_jobs = job_count(&daemon).await;
    let denied = submit_action(
        &daemon,
        "client-viewer",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobSubmit {
            spec: Box::new(test_job_spec(&ws)),
            title: None,
        },
        "matrix-viewer-denied-1",
    )
    .await;
    assert_eq!(error_code(&denied), "project_not_found");
    assert_eq!(job_count(&daemon).await, before_jobs);

    // Outsider (no membership) is denied identically.
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);
    let denied = submit_action(
        &daemon,
        "client-outsider",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobSubmit {
            spec: Box::new(test_job_spec(&ws)),
            title: None,
        },
        "matrix-outsider-denied-1",
    )
    .await;
    assert_eq!(error_code(&denied), "project_not_found");
}

// ── Message/project mismatch ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn message_project_mismatch_creates_nothing() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project_a = ProjectId::new();
    let project_b = ProjectId::new();
    for (project, name, client, role) in [
        (&project_a, "A-Maint", "client-a", ProjectRole::Maintainer),
        (&project_b, "B-Maint", "client-b", ProjectRole::Maintainer),
    ] {
        let member = member_in_project(&team, project, name, client, role).await;
        register_client(&daemon, client, member);
    }
    let channel_a = ensure_default(&daemon, "client-a", project_a.as_str()).await;
    let channel_b = ensure_default(&daemon, "client-b", project_b.as_str()).await;
    let message_b = send(&daemon, "client-b", &channel_b, "other project note").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    // Channel A + message from project B: mismatch, no execution.
    let before = job_count(&daemon).await;
    let response = submit_action(
        &daemon,
        "client-a",
        &channel_a,
        &message_b.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "cross-project task".to_owned(),
            session_id: None,
            title: None,
        },
        "mismatch-1",
    )
    .await;
    assert_eq!(error_code(&response), "chat_project_mismatch");
    assert_eq!(job_count(&daemon).await, before);
    assert_eq!(action_count(&daemon, "client-a", &channel_a).await, 0);
}

// ── Duplicate retransmission ─────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn duplicate_retransmission_creates_one_job() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    let channel = ensure_default(&daemon, "client-maint", project.as_str()).await;
    let message = send(&daemon, "client-maint", &channel, "run this once").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    let first = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "do the thing exactly once".to_owned(),
            session_id: None,
            title: None,
        },
        "dup-key-1",
    )
    .await;
    let (action_a, job_a, dup_a) = action_job_id(&first);
    assert!(!dup_a);

    let second = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "do the thing exactly once".to_owned(),
            session_id: None,
            title: None,
        },
        "dup-key-1",
    )
    .await;
    let (action_b, job_b, dup_b) = action_job_id(&second);
    assert!(dup_b, "retry must converge");
    assert_eq!(action_a, action_b);
    assert_eq!(job_a, job_b);
    assert_eq!(job_count(&daemon).await, 1);

    // Same key reused for a different message is a conflict, not coercion.
    let other_message = send(&daemon, "client-maint", &channel, "other note").await;
    let conflict = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &other_message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "do the thing exactly once".to_owned(),
            session_id: None,
            title: None,
        },
        "dup-key-1",
    )
    .await;
    assert_eq!(error_code(&conflict), "chat_action_conflict");
    assert_eq!(job_count(&daemon).await, 1);
}

// ── Restart preserves idempotency ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_preserves_action_idempotency() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    let channel = ensure_default(&daemon, "client-maint", project.as_str()).await;
    let message = send(&daemon, "client-maint", &channel, "durable work").await;
    let (tmp, ws) = workspace_for(&daemon).await;

    let first = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::ReviewRequest {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "review across restart".to_owned(),
            session_id: None,
            title: None,
        },
        "restart-key-1",
    )
    .await;
    let (action_a, job_a, _) = action_job_id(&first);
    drop(tmp);

    // Fresh daemon over the same pool: composing drops, durable actions stay.
    let restarted = CoreDaemon::new(Some(pool.clone()), None, None);
    let team2 = TeamStore::new(pool);
    let maintainer2 = member_in_project(
        &team2,
        &project,
        "Maintainer2",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    // Re-register the same client id on the restarted daemon.
    register_client(&restarted, "client-maint", maintainer2);
    let second = submit_action(
        &restarted,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::ReviewRequest {
            workspace_id: ws.clone(),
            agent: "reviewer".to_owned(),
            prompt: "review across restart".to_owned(),
            session_id: None,
            title: None,
        },
        "restart-key-1",
    )
    .await;
    let (action_b, job_b, dup_b) = action_job_id(&second);
    assert!(dup_b, "post-restart retry must converge");
    assert_eq!(action_a, action_b);
    assert_eq!(job_a, job_b);
}

// ── Membership removal race ──────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn revoked_membership_creates_nothing() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let principal = human_token(&team, "Temp", "client-temp").await;
    let record = team
        .get_principal(principal.principal_id())
        .await
        .unwrap()
        .unwrap();
    let membership = team
        .create_membership(&project, &record.id, ProjectRole::Maintainer)
        .await
        .unwrap();
    register_client(&daemon, "client-temp", principal);
    let channel = ensure_default(&daemon, "client-temp", project.as_str()).await;
    let message = send(&daemon, "client-temp", &channel, "before revocation").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    // Sanity: allowed before revocation.
    let ok = submit_action(
        &daemon,
        "client-temp",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "allowed work".to_owned(),
            session_id: None,
            title: None,
        },
        "revoke-allow-1",
    )
    .await;
    assert!(matches!(ok, CoreResponse::ChatAction { .. }));
    let before_jobs = job_count(&daemon).await;

    // Revoke, then retry: denied, creates nothing.
    team.revoke_membership(&project, &record.id, membership.revision)
        .await
        .unwrap();
    let denied = submit_action(
        &daemon,
        "client-temp",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "revoked work".to_owned(),
            session_id: None,
            title: None,
        },
        "revoke-denied-1",
    )
    .await;
    assert_eq!(error_code(&denied), "project_not_found");
    assert_eq!(job_count(&daemon).await, before_jobs);
}

// ── Job reference + cancel/failure semantics ─────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn job_reference_and_cancel_follow_canonical_semantics() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    let contributor = member_in_project(
        &team,
        &project,
        "Contributor",
        "client-contrib",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    register_client(&daemon, "client-contrib", contributor);
    let channel = ensure_default(&daemon, "client-contrib", project.as_str()).await;
    let message = send(&daemon, "client-contrib", &channel, "track this job").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    // Contributor submits a generic job through the chat action boundary.
    let submitted = submit_action(
        &daemon,
        "client-contrib",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobSubmit {
            spec: Box::new(test_job_spec(&ws)),
            title: Some("tracked job".to_owned()),
        },
        "ref-submit-1",
    )
    .await;
    let (_, job_id, _) = action_job_id(&submitted);

    // A second message references the existing job (no new execution).
    let ref_message = send(&daemon, "client-contrib", &channel, "see job above").await;
    let before = job_count(&daemon).await;
    let referenced = submit_action(
        &daemon,
        "client-contrib",
        &channel,
        &ref_message.message_id,
        ChatActionSubmitDto::JobReference {
            job_id: job_id.clone(),
            title: Some("link".to_owned()),
        },
        "ref-link-1",
    )
    .await;
    let (_, ref_job, _) = action_job_id(&referenced);
    assert_eq!(ref_job, job_id);
    assert_eq!(
        job_count(&daemon).await,
        before,
        "reference must not execute"
    );

    // Referencing an unknown job fails closed without leaking.
    let unknown = submit_action(
        &daemon,
        "client-contrib",
        &channel,
        &ref_message.message_id,
        ChatActionSubmitDto::JobReference {
            job_id: "job-does-not-exist-1".to_owned(),
            title: None,
        },
        "ref-unknown-1",
    )
    .await;
    assert_eq!(error_code(&unknown), "chat_action_not_found");

    // Canonical cancellation applies to the chat-created job; the chat
    // projection still links to the same job and history is intact.
    match call(
        &daemon,
        "client-maint",
        CoreRequest::JobCancel {
            job_id: job_id.clone(),
            reason: Some("no longer needed".to_owned()),
        },
    )
    .await
    {
        CoreResponse::JobCancelResult { .. } | CoreResponse::Error { .. } => {}
        other => panic!("expected cancel response, got {other:?}"),
    }
    match call(
        &daemon,
        "client-contrib",
        CoreRequest::ChatActionGet {
            channel_id: channel.clone(),
            action_id: match &submitted {
                CoreResponse::ChatAction { action, .. } => action.action_id.clone(),
                other => panic!("expected action, got {other:?}"),
            },
        },
    )
    .await
    {
        CoreResponse::ChatAction { action, .. } => {
            assert_eq!(action.job_id.as_deref(), Some(job_id.as_str()));
        }
        other => panic!("expected action get, got {other:?}"),
    }
    match call(
        &daemon,
        "client-contrib",
        CoreRequest::ChatHistory {
            channel_id: channel.clone(),
            from_seq: None,
            limit: Some(50),
        },
    )
    .await
    {
        CoreResponse::ChatHistory { messages, .. } => {
            assert!(messages.iter().any(|m| m.body == "track this job"));
        }
        other => panic!("expected history, got {other:?}"),
    }
}

// ── Audit causal linkage ─────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn audit_links_message_to_action_to_job() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    let channel = ensure_default(&daemon, "client-maint", project.as_str()).await;
    let message = send(&daemon, "client-maint", &channel, "audited work").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    let submitted = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "audited prompt".to_owned(),
            session_id: None,
            title: Some("audited title".to_owned()),
        },
        "audit-key-1",
    )
    .await;
    let (action_id, job_id, _) = action_job_id(&submitted);

    match call(
        &daemon,
        "client-maint",
        CoreRequest::AuditQuery {
            query: AuditQueryRequestDto {
                project_id: project.as_str().to_owned(),
                action_filter: Some("chat_triggered_action".to_owned()),
                principal_filter: None,
                from_seq: None,
                limit: Some(100),
            },
        },
    )
    .await
    {
        CoreResponse::AuditPage { events, .. } => {
            let serialized = serde_json::to_string(&events).expect("serialize audit");
            assert!(serialized.contains(&channel), "audit must link the channel");
            assert!(
                serialized.contains(&message.message_id),
                "audit must link the message"
            );
            assert!(
                serialized.contains(&action_id),
                "audit must link the action"
            );
            assert!(serialized.contains(&job_id), "audit must link the job");
            assert!(
                !serialized.contains("audited prompt"),
                "prompts must not enter audit"
            );
            assert!(
                !serialized.contains("audited title"),
                "titles must not enter audit"
            );
        }
        other => panic!("expected audit page, got {other:?}"),
    }
}

// ── Secret and reference privacy ─────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn secrets_rejected_and_audit_stays_clean() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    let channel = ensure_default(&daemon, "client-maint", project.as_str()).await;
    let message = send(&daemon, "client-maint", &channel, "clean note").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    // Credential-like prompts are rejected before any durable write.
    let before_jobs = job_count(&daemon).await;
    let rejected = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "deploy api_key=hunter2-secret-value".to_owned(),
            session_id: None,
            title: None,
        },
        "secret-prompt-1",
    )
    .await;
    assert_eq!(error_code(&rejected), "chat_invalid_input");
    assert_eq!(job_count(&daemon).await, before_jobs);

    let rejected_title = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobReference {
            job_id: "job-1".to_owned(),
            title: Some("note token=abc123-secret".to_owned()),
        },
        "secret-title-1",
    )
    .await;
    assert_eq!(error_code(&rejected_title), "chat_invalid_input");

    // ToolProgram jobs cannot be smuggled through the chat boundary.
    let mut evil_spec = test_job_spec(&ws);
    evil_spec.kind = "tool_program".to_owned();
    evil_spec.payload = serde_json::json!({"program_id": "p1"});
    let evil = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::JobSubmit {
            spec: Box::new(evil_spec),
            title: None,
        },
        "evil-tool-program-1",
    )
    .await;
    assert_eq!(error_code(&evil), "chat_invalid_input");
    assert_eq!(job_count(&daemon).await, before_jobs);

    // Audit pages carry no secret material.
    match call(
        &daemon,
        "client-maint",
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
            let serialized = serde_json::to_string(&events).expect("serialize audit");
            assert!(!serialized.contains("hunter2-secret-value"));
        }
        other => panic!("expected audit page, got {other:?}"),
    }
}

// ── Action listing and status projection ─────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn action_list_projects_status_without_duplicating_execution() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    let maintainer = member_in_project(
        &team,
        &project,
        "Maintainer",
        "client-maint",
        ProjectRole::Maintainer,
    )
    .await;
    register_client(&daemon, "client-maint", maintainer);
    let channel = ensure_default(&daemon, "client-maint", project.as_str()).await;
    let message = send(&daemon, "client-maint", &channel, "status check").await;
    let (_tmp, ws) = workspace_for(&daemon).await;

    let submitted = submit_action(
        &daemon,
        "client-maint",
        &channel,
        &message.message_id,
        ChatActionSubmitDto::AgentTask {
            workspace_id: ws.clone(),
            agent: "worker".to_owned(),
            prompt: "status work".to_owned(),
            session_id: None,
            title: Some("status title".to_owned()),
        },
        "status-key-1",
    )
    .await;
    let (action_id, job_id, _) = action_job_id(&submitted);

    match call(
        &daemon,
        "client-maint",
        CoreRequest::ChatActionList {
            channel_id: channel.clone(),
            message_id: Some(message.message_id.clone()),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::ChatActionList { actions, .. } => {
            assert_eq!(actions.len(), 1);
            assert_eq!(actions[0].action_id, action_id);
            assert_eq!(actions[0].job_id.as_deref(), Some(job_id.as_str()));
            assert_eq!(actions[0].status, "submitted");
        }
        other => panic!("expected action list, got {other:?}"),
    }

    match call(
        &daemon,
        "client-maint",
        CoreRequest::ChatActionGet {
            channel_id: channel.clone(),
            action_id: action_id.clone(),
        },
    )
    .await
    {
        CoreResponse::ChatAction { action, .. } => {
            assert_eq!(action.job_id.as_deref(), Some(job_id.as_str()));
        }
        other => panic!("expected action get, got {other:?}"),
    }
}
