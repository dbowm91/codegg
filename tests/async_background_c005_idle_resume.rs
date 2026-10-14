//! C005 production idle-resume qualification (authenticated grant +
//! delegated executor slice, ADR-0031 accepted).
//!
//! Proves, against a real `CoreDaemon` with a stub `TurnRuntime` at the
//! provider boundary (no inference, no network), that:
//!
//! - An authenticated owner approves exactly one bounded continuation per
//!   background wait through the daemon gate (`ContinuationGrantCreate` /
//!   `Revoke` / `Status`): server-resolved principal/session/workspace,
//!   immutable origin attribution, structural audit, privacy-safe denials.
//! - On the durable terminal the daemon admits exactly one canonical owner
//!   turn through the real `TurnSubmit` admission (receipt, controller
//!   lease held by the owner, `TurnStarted`, origin capture, budgets,
//!   active-turn exclusivity) with the bounded host-authored terminal
//!   result — via the production fanout wake, never harness dispatch.
//! - Notify-only and every unapproved/expired/revoked/mismatched/legacy
//!   case yields one notice + live hint and zero turns; ambiguity after
//!   admission becomes `needs_attention`, never a replay.
//! - Exactly-once holds under a 50-way concurrent wake race and across
//!   the restart-reconciliation twin.
//!
//! Strictly forbidden in this file: `FakeContinuationExecutor`,
//! `IdleContinuationCoordinator::maybe_dispatch`, `TurnSubmit` composed by
//! the test (the delegated entry composes daemon-side), model-supplied
//! identity, or any fake `client_id`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use codegg::agent::turn_runtime::{TurnRunInput, TurnRunOutput, TurnRuntime};
use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreEvent, CoreRequest, CoreResponse};
use codegg::scheduler::idle_continuation::{DispatchOutcome, IdleContinuationState};
use codegg_core::jobs::{JobId, JobKind, JobPriority, JobRecord, JobSource, JobState};
use codegg_core::workspace::WorkspaceId;

mod common;

// ── Stub provider boundary ─────────────────────────────────────────────

#[derive(Debug, Clone)]
struct StubCall {
    session_id: String,
    model: String,
    turn_id: String,
    text: String,
}

#[derive(Default)]
struct StubTurnRuntime {
    calls: Mutex<Vec<StubCall>>,
    fail: Mutex<bool>,
}

impl StubTurnRuntime {
    fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    fn calls(&self) -> Vec<StubCall> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl TurnRuntime for StubTurnRuntime {
    async fn run_turn(
        &self,
        input: TurnRunInput,
    ) -> Result<TurnRunOutput, codegg::error::AppError> {
        let mut text = String::new();
        for message in &input.messages_dto {
            if let codegg::protocol::dto::ProviderMessage::User { content } = message {
                for part in content {
                    if let codegg::protocol::dto::ContentPart::Text { text: chunk } = part {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(chunk);
                    }
                }
            }
        }
        // Record the provider-boundary entry even when the attempt below
        // reports ambiguity: the entry happened, the outcome did not.
        self.calls.lock().unwrap().push(StubCall {
            session_id: input.session_id.clone(),
            model: input.model.clone(),
            turn_id: input.turn_id.clone(),
            text,
        });
        if *self.fail.lock().unwrap() {
            return Err(codegg::error::AppError::Other(anyhow::anyhow!(
                "stub provider-side start ambiguity"
            )));
        }
        Ok(TurnRunOutput {
            cancel_tx: tokio::sync::watch::channel(false).0,
            steer_tx: tokio::sync::mpsc::channel(8).0,
        })
    }
}

// ── Harness ────────────────────────────────────────────────────────────

async fn stub_daemon(pool: sqlx::SqlitePool) -> (CoreDaemon, Arc<StubTurnRuntime>) {
    let stub = Arc::new(StubTurnRuntime::default());
    let mut deps = CoreRuntimeDeps::new(Some(pool), None, None);
    deps.turn_runtime = stub.clone();
    (CoreDaemon::with_deps(deps), stub)
}

/// Multi-connection isolated pool for the wake race. The shared
/// `isolated_pool` serializes on one connection, which starves racing
/// wakes into pool-acquire timeouts — a harness artifact, not the CAS
/// logic under test. Production daemon pools are multi-connection, so
/// the race runs against the same shape.
async fn race_pool() -> sqlx::SqlitePool {
    use std::str::FromStr;
    let name = format!("codegg_test_c005_race_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = sqlx::sqlite::SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite connect options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
        .expect("connect to in-memory sqlite");
    codegg::session::schema::migrate(&pool)
        .await
        .expect("run migrations");
    pool
}

async fn seed_context(daemon: &CoreDaemon, root: &std::path::Path) -> (String, String) {
    let workspace = match daemon
        .handle_request(new_request(
            "req-ws-reg".into(),
            CoreRequest::WorkspaceRegister {
                root: root.to_string_lossy().into_owned(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project = codegg_core::project_catalog::ProjectCatalog::new(
        daemon.pool.clone().expect("test daemon pool"),
    )
    .register_local_project(
        codegg_core::project_catalog::RegisterLocalProject {
            display_name: "C005 test project".to_string(),
            description: None,
            tags: Vec::new(),
            primary_repository_id: None,
        },
        &codegg_core::workspace::WorkspaceId::new_unchecked(&workspace),
        "c005-test",
    )
    .await
    .expect("test project registration");
    (project.project_id.as_str().to_string(), workspace)
}

async fn create_session(
    daemon: &CoreDaemon,
    dir: &std::path::Path,
    project_id: &str,
    workspace_id: &str,
) -> String {
    match daemon
        .handle_request(new_request(
            "req-session-create".into(),
            CoreRequest::SessionCreate {
                directory: dir.to_string_lossy().into_owned(),
                title: None,
                project_id: Some(project_id.to_owned()),
                workspace_id: Some(workspace_id.to_owned()),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("session create failed: {other:?}"),
    }
}

/// Seed one healthy `openai` connection plus a `gpt-4o` catalog entry and
/// pin the session selection to it through the real client request path.
async fn seed_selection(daemon: &CoreDaemon, pool: &sqlx::SqlitePool, session_id: &str) -> String {
    use codegg_core::identity::PrincipalId;
    use codegg_core::provider_connections::{
        Endpoint, NewProviderConnection, ProviderConnectionStore, ProviderKind, ProviderScope,
        SecretBindingLocator, SecretRef, TlsPolicy,
    };
    let conn_store = ProviderConnectionStore::new(pool.clone());
    let conn_id = conn_store
        .create(NewProviderConnection {
            provider_kind: ProviderKind::OpenAi,
            display_name: "C005 test connection".to_string(),
            endpoint: Endpoint::new("http://c005.example.com", TlsPolicy::Disabled).unwrap(),
            tls_policy: TlsPolicy::Disabled,
            scope: ProviderScope::personal(PrincipalId::parse("test-user").unwrap()),
            secret_binding: Some(
                SecretBindingLocator::new(SecretRef::new(), "c005-provider", "acct-c005").unwrap(),
            ),
        })
        .await
        .expect("create connection")
        .id;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    sqlx::query(
        "INSERT INTO provider_connection_health \
         (connection_id, revision, status, duration_ms, checked_at, catalog_revision) \
         VALUES (?, 1, 'healthy', 10, ?, ?)",
    )
    .bind(conn_id.as_str())
    .bind(now)
    .bind("cat-c005")
    .execute(pool)
    .await
    .expect("seed health");
    sqlx::query(
        "INSERT INTO provider_connection_models \
         (connection_id, revision, model_id, model_name, context_window, \
          max_output_tokens, supports_tools, supports_vision) \
         VALUES (?, 1, 'gpt-4o', 'GPT-4o', 128000, 16384, 1, 1)",
    )
    .bind(conn_id.as_str())
    .execute(pool)
    .await
    .expect("seed model");
    match daemon
        .handle_request(new_request(
            "req-select".into(),
            CoreRequest::SessionSelectionUpdate {
                request: Box::new(codegg::protocol::provider::UpdateSessionSelectionRequest {
                    session_id: session_id.to_owned(),
                    connection_id: conn_id.as_str().to_owned(),
                    model_id: "gpt-4o".to_owned(),
                    expected_connection_revision: None,
                    expected_catalog_revision: None,
                }),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::SessionSelectionUpdated { .. } => conn_id.as_str().to_owned(),
        other => panic!("selection update failed: {other:?}"),
    }
}

fn job_record(job_id: &str, session: Option<&str>, state: JobState) -> JobRecord {
    let now = chrono::Utc::now();
    JobRecord {
        job_id: JobId::new_unchecked(job_id),
        workspace_id: WorkspaceId::new_unchecked("ws-c005"),
        session_id: session.map(ToOwned::to_owned),
        turn_id: None,
        kind: JobKind::Test,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload: codegg_core::jobs::JobPayload::Test {
            command: "cargo test".into(),
            argv: vec!["cargo".into(), "test".into()],
            cwd: None,
            scope: None,
            parent_run_id: None,
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Test),
        state,
        current_attempt_id: None,
        attempt_count: 0,
        created_at: now,
        updated_at: now,
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        labels: Default::default(),
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: Default::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::SafeRepeat,
    }
}

async fn grant_create(
    daemon: &CoreDaemon,
    request_id: &str,
    session_id: &str,
    notice_job_id: &str,
) -> CoreResponse {
    daemon
        .handle_request(new_request(
            request_id.to_owned(),
            CoreRequest::ContinuationGrantCreate {
                session_id: session_id.to_owned(),
                notice_job_id: notice_job_id.to_owned(),
                allowed_model: None,
                allowed_connection: None,
                max_continuations: None,
                max_tokens: None,
                max_tool_calls: None,
                max_duration_ms: None,
                ttl_ms: None,
            },
        ))
        .await
        .unwrap()
}

fn grant_id_of(response: &CoreResponse) -> String {
    match response {
        CoreResponse::ContinuationGrantCreated { grant_id } => grant_id.clone(),
        other => panic!("expected grant creation, got {other:?}"),
    }
}

async fn record_completed(
    daemon: &CoreDaemon,
    job_id: &str,
    session_id: &str,
    summary: &str,
) -> codegg::scheduler::job_notifications::JobTerminalNotice {
    let notifier = daemon
        .deps
        .job_notification_service
        .clone()
        .expect("daemon owns a notifier");
    notifier
        .subscribe(
            &JobId::new_unchecked(job_id),
            "test",
            session_id,
            None,
            &format!("inv-{job_id}"),
        )
        .await
        .expect("subscribe");
    notifier
        .record_terminal(
            &job_record(job_id, Some(session_id), JobState::Completed),
            summary,
            None,
        )
        .await
        .expect("record")
        .expect("subscribed terminal produces a notice")
}

async fn wait_intent_state(
    daemon: &CoreDaemon,
    notice_id: &str,
    grant_id: &str,
    want: IdleContinuationState,
) -> codegg::scheduler::idle_continuation::IdleContinuationIntent {
    let service = daemon
        .continuation_service
        .clone()
        .expect("daemon owns continuation store");
    let deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    loop {
        if let Ok(Some(intent)) = service.get_intent(notice_id, grant_id).await {
            if intent.state == want {
                return intent;
            }
            if matches!(
                intent.state,
                IdleContinuationState::NeedsAttention
                    | IdleContinuationState::Completed
                    | IdleContinuationState::Suppressed
            ) && intent.state != want
            {
                panic!(
                    "intent reached terminal {:?}, wanted {:?}",
                    intent.state, want
                );
            }
        }
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for intent {want:?}"),
            _ = tokio::time::sleep(Duration::from_millis(25)) => {}
        }
    }
}

// ── Grant workflow ─────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn grant_create_status_revoke_roundtrip_is_server_resolved() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, _stub) = stub_daemon(pool).await;
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;

    let created = grant_create(&daemon, "req-grant-1", &session_id, "job-c005-1").await;
    let grant_id = grant_id_of(&created);

    // Status: server-resolved identity, approved, bounded.
    let dto = match daemon
        .handle_request(new_request(
            "req-grant-status".into(),
            CoreRequest::ContinuationGrantStatus {
                session_id: session_id.clone(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantStatus { grant: Some(dto) } => dto,
        other => panic!("expected grant status, got {other:?}"),
    };
    assert_eq!(dto.session_id, session_id);
    assert_eq!(dto.notice_job_id, "job-c005-1");
    assert!(!dto.owner_principal.is_empty(), "owner is server-resolved");
    assert_eq!(dto.status, "approved");
    assert_eq!(dto.max_continuations, 1);
    assert!(dto.expiry_ms > chrono::Utc::now().timestamp_millis());

    // Attestable origin exists (the wake-eligibility root).
    let attribution =
        codegg_core::authorization::OriginAttributionStore::new(daemon.pool.clone().expect("pool"))
            .get("continuation_grant", &grant_id)
            .await
            .expect("attribution read")
            .expect("host grant captures origin");
    assert_eq!(attribution.origin_principal.as_str(), dto.owner_principal);

    // Revoke: idempotent, then visible in status.
    match daemon
        .handle_request(new_request(
            "req-grant-revoke".into(),
            CoreRequest::ContinuationGrantRevoke {
                session_id: session_id.clone(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantRevoked { grant_id: got } => assert_eq!(got, grant_id),
        other => panic!("expected revocation, got {other:?}"),
    }
    match daemon
        .handle_request(new_request(
            "req-grant-revoke-again".into(),
            CoreRequest::ContinuationGrantRevoke {
                session_id: session_id.clone(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantRevoked { .. } => {}
        other => panic!("revocation must be idempotent, got {other:?}"),
    }
    match daemon
        .handle_request(new_request(
            "req-grant-status-2".into(),
            CoreRequest::ContinuationGrantStatus {
                session_id: session_id.clone(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantStatus { grant: Some(dto) } => {
            assert_eq!(dto.status, "revoked")
        }
        other => panic!("expected revoked status, got {other:?}"),
    }

    // Privacy-safe: unknown grants and foreign sessions read as absent.
    match daemon
        .handle_request(new_request(
            "req-grant-status-unknown".into(),
            CoreRequest::ContinuationGrantStatus {
                session_id: session_id.clone(),
                grant_id: "cg-does-not-exist".into(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantStatus { grant: None } => {}
        other => panic!("unknown grant must read as absent, got {other:?}"),
    }
    match daemon
        .handle_request(new_request(
            "req-grant-status-foreign".into(),
            CoreRequest::ContinuationGrantStatus {
                session_id: "sess-foreign".into(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantStatus { grant: None } => {}
        other => panic!("foreign session must read as absent, got {other:?}"),
    }
    match daemon
        .handle_request(new_request(
            "req-grant-revoke-foreign".into(),
            CoreRequest::ContinuationGrantRevoke {
                session_id: "sess-foreign".into(),
                grant_id: grant_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::Error { code, .. } => assert_eq!(code, "continuation_grant_not_found"),
        other => panic!("foreign revoke must not signal existence, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn viewer_is_denied_grant_creation_with_zero_side_effect() {
    use codegg_core::team::{PrincipalKind, TeamStore};
    use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

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

    let pool = common::pool::isolated_pool().await;
    let (daemon, _stub) = stub_daemon(pool.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let workspace_id = match daemon
        .handle_request(new_request(
            "req-ws".into(),
            CoreRequest::WorkspaceRegister {
                root: dir.path().to_string_lossy().into_owned(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace register failed: {other:?}"),
    };
    let project_id = match daemon
        .handle_request(new_request(
            "req-proj".into(),
            CoreRequest::ProjectRegister {
                request: codegg::protocol::dto::ProjectRegisterRequestDto {
                    workspace_id: workspace_id.clone(),
                    display_name: "c005 team project".to_owned(),
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
        other => panic!("project register failed: {other:?}"),
    };
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;

    let team = TeamStore::new(pool);
    let project = codegg_core::identity::ProjectId::parse(&project_id).expect("project id");
    let viewer = human_token(&team, "C005Viewer", "client-c005-viewer").await;
    let viewer_record = team
        .get_principal(viewer.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &project,
        &viewer_record.id,
        codegg_core::authorization::ProjectRole::Viewer,
    )
    .await
    .unwrap();
    let controller = human_token(&team, "C005Contributor", "client-c005-contrib").await;
    let controller_record = team
        .get_principal(controller.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(
        &project,
        &controller_record.id,
        codegg_core::authorization::ProjectRole::Contributor,
    )
    .await
    .unwrap();
    daemon.clients.register_with_principal(
        "client-c005-viewer".to_owned(),
        "viewer".to_owned(),
        None,
        viewer,
    );
    daemon.clients.register_with_principal(
        "client-c005-contrib".to_owned(),
        "contrib".to_owned(),
        None,
        controller,
    );

    // Viewer (read-only) cannot approve a future turn: denied at the gate
    // with zero side effect (no grant row, no attribution row).
    match daemon
        .handle_request_for_client(
            new_request(
                "req-viewer-grant".into(),
                CoreRequest::ContinuationGrantCreate {
                    session_id: session_id.clone(),
                    notice_job_id: "job-c005-viewer".into(),
                    allowed_model: None,
                    allowed_connection: None,
                    max_continuations: None,
                    max_tokens: None,
                    max_tool_calls: None,
                    max_duration_ms: None,
                    ttl_ms: None,
                },
            ),
            "client-c005-viewer",
        )
        .await
        .unwrap()
    {
        CoreResponse::Error { .. } => {}
        other => panic!("viewer grant creation must be denied, got {other:?}"),
    }
    let service = daemon.continuation_service.clone().expect("store");
    assert!(
        service
            .grant_for_job("job-c005-viewer")
            .await
            .expect("grant read")
            .is_none(),
        "denied grant must leave no row"
    );

    // Contributor (agent.invoke) approves through the same gate.
    match daemon
        .handle_request_for_client(
            new_request(
                "req-contrib-grant".into(),
                CoreRequest::ContinuationGrantCreate {
                    session_id: session_id.clone(),
                    notice_job_id: "job-c005-contrib".into(),
                    allowed_model: None,
                    allowed_connection: None,
                    max_continuations: None,
                    max_tokens: None,
                    max_tool_calls: None,
                    max_duration_ms: None,
                    ttl_ms: None,
                },
            ),
            "client-c005-contrib",
        )
        .await
        .unwrap()
    {
        CoreResponse::ContinuationGrantCreated { .. } => {}
        other => panic!("contributor grant creation must succeed, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn grant_create_rejects_unknown_and_archived_sessions() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, _stub) = stub_daemon(pool).await;
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;

    match grant_create(&daemon, "req-grant-unknown", "sess-missing", "job-x").await {
        CoreResponse::Error { code, .. } => assert_eq!(code, "invalid_continuation_grant"),
        other => panic!("unknown session must fail closed, got {other:?}"),
    }

    daemon
        .handle_request(new_request(
            "req-archive".into(),
            CoreRequest::SessionArchive {
                session_id: session_id.clone(),
                unarchive: false,
            },
        ))
        .await
        .unwrap();
    match grant_create(&daemon, "req-grant-archived", &session_id, "job-x").await {
        CoreResponse::Error { code, .. } => assert_eq!(code, "invalid_continuation_grant"),
        other => panic!("archived session must fail closed, got {other:?}"),
    }
}

// ── Production resume ──────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn prod_resume_admits_exactly_one_owner_turn_through_fanout() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, stub) = stub_daemon(pool.clone()).await;
    let daemon = Arc::new(daemon);
    // The serving daemon owns its delegated wake (production wiring).
    daemon.attach_idle_wake();
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;
    let connection_id = seed_selection(&daemon, &pool, &session_id).await;

    let grant_id = grant_id_of(
        &grant_create(&daemon, "req-grant-resume", &session_id, "job-c005-resume").await,
    );

    // Subscribe to the live log BEFORE the terminal commits.
    let mut rx = daemon.event_log.subscribe();
    let notice = record_completed(&daemon, "job-c005-resume", &session_id, "build green").await;

    // The production fanout wake admits exactly one turn: receipt durable,
    // controller held by the owner, TurnStarted published.
    let intent = wait_intent_state(
        &daemon,
        &notice.notice_id,
        &grant_id,
        IdleContinuationState::Completed,
    )
    .await;
    let turn_id = intent.turn_id.expect("completed intent records its turn");

    assert_eq!(stub.call_count(), 1, "exactly one provider-boundary entry");
    let call = stub.calls().pop().expect("one call");
    assert_eq!(call.session_id, session_id);
    assert_eq!(call.turn_id, turn_id);
    assert!(
        call.model.contains("gpt-4o"),
        "grant-unbound wake keeps the session selection, got {}",
        call.model
    );
    assert!(
        call.text.contains("build green"),
        "turn carries the host-authored terminal result, got {:?}",
        call.text
    );
    assert!(
        !call.text.contains("http"),
        "no remote locator may enter the prompt, got {:?}",
        call.text
    );

    // Controller lease held by the grant owner (not the daemon identity).
    let store = daemon
        .continuation_service
        .clone()
        .expect("store")
        .get_grant(&grant_id)
        .await
        .expect("grant read")
        .expect("grant row");
    assert_eq!(store.used_continuations, 1);
    assert_eq!(
        store.status,
        codegg::scheduler::idle_continuation::ContinuationGrantStatus::Consumed
    );

    // TurnStarted reached the live log for the owner session.
    let deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("timed out waiting for TurnStarted {turn_id}"),
            recv = rx.recv() => {
                let env = recv.expect("event log sender must stay alive");
                if let CoreEvent::TurnStarted { turn_id: got, .. } = &env.payload {
                    if got == &turn_id {
                        break;
                    }
                }
            }
        }
    }

    // M003 origin capture on the admitted turn.
    let turn_attribution = codegg_core::authorization::OriginAttributionStore::new(pool)
        .get("turn", &turn_id)
        .await
        .expect("attribution read")
        .expect("delegated turn captures origin");
    assert_eq!(
        turn_attribution.origin_principal.as_str(),
        store.owner_principal.as_deref().unwrap_or_default()
    );

    // A second wake for the same terminal replays, never a second turn.
    assert_eq!(
        daemon.maybe_dispatch_idle_continuation(&notice).await,
        DispatchOutcome::AlreadyDispatched
    );
    assert_eq!(stub.call_count(), 1);
    let _ = connection_id;
}

#[tokio::test(flavor = "current_thread")]
async fn notify_only_without_grant_publishes_hint_and_starts_no_turn() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, stub) = stub_daemon(pool).await;
    let daemon = Arc::new(daemon);
    daemon.attach_idle_wake();
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;

    let mut rx = daemon.event_log.subscribe();
    let notice = record_completed(&daemon, "job-c005-notify", &session_id, "build green").await;

    // Live hint still publishes (delivery preserved) …
    let deadline = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => panic!("hint must publish without a grant"),
            recv = rx.recv() => {
                let env = recv.expect("event log sender must stay alive");
                if let CoreEvent::BackgroundJobCompleted { notice_id, .. } = &env.payload {
                    if notice_id == &notice.notice_id {
                        break;
                    }
                }
            }
        }
    }
    // … and no turn starts, with no intent row at all.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(stub.call_count(), 0);
    let service = daemon.continuation_service.clone().expect("store");
    assert!(service
        .intents_for_notice(&notice.notice_id)
        .await
        .expect("intents read")
        .is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn revoked_and_expired_grants_stay_notify_only() {
    for (name, ttl) in [("revoked", None), ("expired", Some(1))] {
        let pool = common::pool::isolated_pool().await;
        let (daemon, stub) = stub_daemon(pool).await;
        let daemon = Arc::new(daemon);
        daemon.attach_idle_wake();
        let dir = tempfile::tempdir().unwrap();
        let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
        let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;
        seed_selection(&daemon, &daemon.pool.clone().expect("pool"), &session_id).await;

        let job = format!("job-c005-{name}");
        let grant_response = if ttl.is_some() {
            daemon
                .handle_request(new_request(
                    "req-grant-ttl".into(),
                    CoreRequest::ContinuationGrantCreate {
                        session_id: session_id.clone(),
                        notice_job_id: job.clone(),
                        allowed_model: None,
                        allowed_connection: None,
                        max_continuations: None,
                        max_tokens: None,
                        max_tool_calls: None,
                        max_duration_ms: None,
                        ttl_ms: ttl,
                    },
                ))
                .await
                .unwrap()
        } else {
            grant_create(&daemon, "req-grant-rev", &session_id, &job).await
        };
        let grant_id = grant_id_of(&grant_response);
        if ttl.is_none() {
            daemon
                .handle_request(new_request(
                    "req-revoke".into(),
                    CoreRequest::ContinuationGrantRevoke {
                        session_id: session_id.clone(),
                        grant_id: grant_id.clone(),
                    },
                ))
                .await
                .unwrap();
        } else {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let notice = record_completed(&daemon, &job, &session_id, "build red").await;
        // Settle: the attached wake evaluates and must suppress.
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(stub.call_count(), 0, "{name} grant must start no turn");
        let service = daemon.continuation_service.clone().expect("store");
        let intents = service
            .intents_for_notice(&notice.notice_id)
            .await
            .expect("intents read");
        assert!(
            intents
                .iter()
                .all(|intent| intent.state != IdleContinuationState::Completed),
            "{name} grant must never complete an intent"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn binding_mismatches_and_legacy_rows_stay_notify_only() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, stub) = stub_daemon(pool.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;
    seed_selection(&daemon, &pool, &session_id).await;
    let service = daemon.continuation_service.clone().expect("store");

    // Wrong-model binding.
    let mismatched = daemon
        .handle_request(new_request(
            "req-grant-model".into(),
            CoreRequest::ContinuationGrantCreate {
                session_id: session_id.clone(),
                notice_job_id: "job-c005-model".into(),
                allowed_model: Some("no-such-model".into()),
                allowed_connection: None,
                max_continuations: None,
                max_tokens: None,
                max_tool_calls: None,
                max_duration_ms: None,
                ttl_ms: None,
            },
        ))
        .await
        .unwrap();
    let model_grant = grant_id_of(&mismatched);

    // Wrong-connection binding.
    let mismatched_conn = daemon
        .handle_request(new_request(
            "req-grant-conn".into(),
            CoreRequest::ContinuationGrantCreate {
                session_id: session_id.clone(),
                notice_job_id: "job-c005-conn".into(),
                allowed_model: None,
                allowed_connection: Some("conn-does-not-exist".into()),
                max_continuations: None,
                max_tokens: None,
                max_tool_calls: None,
                max_duration_ms: None,
                ttl_ms: None,
            },
        ))
        .await
        .unwrap();
    let conn_grant = grant_id_of(&mismatched_conn);

    // Legacy caller-supplied row: approved strings, zero attestable origin.
    let legacy = service
        .authorize(codegg::scheduler::idle_continuation::NewGrant {
            notice_job_id: "job-c005-legacy".into(),
            session_id: session_id.clone(),
            owner_principal: Some("local-owner".into()),
            workspace_id: None,
            origin_turn_id: None,
            allowed_model: None,
            allowed_connection: None,
            max_continuations: Some(1),
            max_tokens: None,
            max_tool_calls: None,
            max_duration_ms: None,
            ttl_ms: None,
        })
        .await
        .expect("legacy row persists");

    for (job, grant_id) in [
        ("job-c005-model", model_grant),
        ("job-c005-conn", conn_grant),
        ("job-c005-legacy", legacy.grant_id.clone()),
    ] {
        let notice = record_completed(&daemon, job, &session_id, "build green").await;
        // Unattached daemon: no fanout wake. Drive the production entry
        // directly — it is what the fanout invokes, not harness code.
        let outcome = daemon.maybe_dispatch_idle_continuation(&notice).await;
        assert_eq!(
            outcome,
            DispatchOutcome::Suppressed,
            "{job} must fail closed, grant {grant_id}"
        );
    }
    assert_eq!(stub.call_count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn fifty_way_wake_race_admits_exactly_one_turn() {
    let pool = race_pool().await;
    let (daemon, stub) = stub_daemon(pool.clone()).await;
    let daemon = Arc::new(daemon);
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;
    seed_selection(&daemon, &pool, &session_id).await;
    let grant_id =
        grant_id_of(&grant_create(&daemon, "req-grant-race", &session_id, "job-c005-race").await);
    // Unattached: the fanout never wakes, so all 50 dispatches below are
    // the racing callers (twin + fanout + redelivery overlap).
    let notice = record_completed(&daemon, "job-c005-race", &session_id, "build green").await;

    let mut handles = Vec::new();
    for _ in 0..50 {
        let daemon = daemon.clone();
        let notice = notice.clone();
        handles.push(tokio::spawn(async move {
            daemon.maybe_dispatch_idle_continuation(&notice).await
        }));
    }
    let mut dispatched = 0;
    let mut suppressed_losers = 0;
    for handle in handles {
        match handle.await.expect("wake task") {
            DispatchOutcome::Dispatched => dispatched += 1,
            DispatchOutcome::AlreadyDispatched => {}
            // A pre-claim transient storage error (pool starvation under
            // parallel load) reports suppression without writing any row,
            // so a later wake converges to replay. The count below must
            // stay small, and convergence + single dispatch must hold.
            DispatchOutcome::Suppressed => suppressed_losers += 1,
            other => panic!("race loser must report replay, got {other:?}"),
        }
    }
    assert_eq!(dispatched, 1, "exactly one wake wins the intent CAS");
    assert_eq!(stub.call_count(), 1, "exactly one provider-boundary entry");
    assert!(
        suppressed_losers <= 5,
        "transient suppression must stay rare, got {suppressed_losers}"
    );
    // Convergence: every loser state resolves to replay afterwards.
    assert_eq!(
        daemon.maybe_dispatch_idle_continuation(&notice).await,
        DispatchOutcome::AlreadyDispatched
    );
    let intent = wait_intent_state(
        &daemon,
        &notice.notice_id,
        &grant_id,
        IdleContinuationState::Completed,
    )
    .await;
    assert!(intent.turn_id.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn ambiguous_provider_start_becomes_needs_attention_without_replay() {
    let pool = common::pool::isolated_pool().await;
    let (daemon, stub) = stub_daemon(pool.clone()).await;
    let daemon = Arc::new(daemon);
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon, dir.path()).await;
    let session_id = create_session(&daemon, dir.path(), &project_id, &workspace_id).await;
    seed_selection(&daemon, &pool, &session_id).await;
    let grant_id =
        grant_id_of(&grant_create(&daemon, "req-grant-amb", &session_id, "job-c005-amb").await);
    *stub.fail.lock().unwrap() = true;
    let notice = record_completed(&daemon, "job-c005-amb", &session_id, "build green").await;

    assert_eq!(
        daemon.maybe_dispatch_idle_continuation(&notice).await,
        DispatchOutcome::NeedsAttention
    );
    // A second wake observes the ambiguity; it never replays the turn.
    assert_eq!(
        daemon.maybe_dispatch_idle_continuation(&notice).await,
        DispatchOutcome::NeedsAttention
    );
    assert_eq!(stub.call_count(), 1);
    wait_intent_state(
        &daemon,
        &notice.notice_id,
        &grant_id,
        IdleContinuationState::NeedsAttention,
    )
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn restart_twin_heals_a_missed_wake_exactly_once() {
    let pool = common::pool::isolated_pool().await;
    let (daemon_a, stub_a) = stub_daemon(pool.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon_a, dir.path()).await;
    let session_id = create_session(&daemon_a, dir.path(), &project_id, &workspace_id).await;
    seed_selection(&daemon_a, &pool, &session_id).await;
    let grant_id =
        grant_id_of(&grant_create(&daemon_a, "req-grant-twin", &session_id, "job-c005-twin").await);
    // Unattached daemon A: the terminal commits and the hint publishes,
    // but no wake ever evaluates (the missed-wake crash shape).
    let notice = record_completed(&daemon_a, "job-c005-twin", &session_id, "build green").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(stub_a.call_count(), 0);

    // A restarted daemon over the same pool heals the missed wake through
    // the bounded restart twin — exactly once, then quiescent.
    let (daemon_b, stub_b) = stub_daemon(pool).await;
    assert_eq!(daemon_b.reconcile_idle_continuation_wakes().await, 1);
    assert_eq!(stub_b.call_count(), 1);
    wait_intent_state(
        &daemon_b,
        &notice.notice_id,
        &grant_id,
        IdleContinuationState::Completed,
    )
    .await;
    assert_eq!(daemon_b.reconcile_idle_continuation_wakes().await, 0);
    assert_eq!(stub_b.call_count(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn restart_with_claimed_intent_reports_replay_without_readmitting() {
    let pool = common::pool::isolated_pool().await;
    let (daemon_a, stub_a) = stub_daemon(pool.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let (project_id, workspace_id) = seed_context(&daemon_a, dir.path()).await;
    let session_id = create_session(&daemon_a, dir.path(), &project_id, &workspace_id).await;
    seed_selection(&daemon_a, &pool, &session_id).await;
    let grant_id = grant_id_of(
        &grant_create(&daemon_a, "req-grant-claim", &session_id, "job-c005-claim").await,
    );
    // Unattached daemon A: terminal commits, hint publishes, no wake.
    let notice = record_completed(&daemon_a, "job-c005-claim", &session_id, "build green").await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(stub_a.call_count(), 0);

    // Crash between intent claim and admission: a durable `claimed`
    // intent with no turn, as a killed wake would leave it. Seeded
    // through SQL because no production path mints claims without
    // admitting (the claim and the admission are one entry).
    let now = chrono::Utc::now().timestamp_millis();
    let intent_id = format!("{}+{}", notice.notice_id, grant_id);
    let record = serde_json::json!({
        "intent_id": intent_id,
        "notice_id": notice.notice_id,
        "grant_id": grant_id,
        "session_id": session_id,
        "state": "claimed",
        "turn_id": null,
        "cause": "completed_background_job",
        "created_at": now,
        "updated_at": now,
    });
    sqlx::query(
        "INSERT INTO idle_continuation (intent_id, notice_id, grant_id, session_id, state, turn_id, cause, created_at, updated_at, record_json) \
         VALUES (?, ?, ?, ?, 'claimed', NULL, 'completed_background_job', ?, ?, ?)",
    )
    .bind(&intent_id)
    .bind(&notice.notice_id)
    .bind(&grant_id)
    .bind(&session_id)
    .bind(now)
    .bind(now)
    .bind(record.to_string())
    .execute(&pool)
    .await
    .expect("seed claimed intent");

    // A restarted daemon never re-admits a claimed intent: the twin
    // skips it, and a direct wake reports replay. The ambiguity is
    // explicit (claimed, no turn) rather than a silent second admission.
    let (daemon_b, stub_b) = stub_daemon(pool).await;
    assert_eq!(daemon_b.reconcile_idle_continuation_wakes().await, 0);
    assert_eq!(
        daemon_b.maybe_dispatch_idle_continuation(&notice).await,
        DispatchOutcome::AlreadyDispatched
    );
    assert_eq!(stub_b.call_count(), 0);
}
