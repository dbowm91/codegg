//! M001 — explicit target job and schedule boundary.
//!
//! Boundary proof for the plan: a client may name a configured Eggwork
//! node for a job or a schedule, and the daemon admits that target only
//! when every gate passes.
//!
//! - A remote target names a node; it never carries an endpoint, a key
//!   path, or any other credential-bearing locator.
//! - An omitted target is the local executor, exactly as before.
//! - A malformed target is a typed refusal, never a silent fallback to
//!   local execution and never a dropped request.
//! - A refused remote submission creates no job at all — not even a
//!   local one.
//! - A refused remote schedule creates no schedule, so it can never fire
//!   a local job later.
//! - The administrator gate and the principal gate are independent: a
//!   contributor who may submit jobs still cannot name a remote node,
//!   and a node the administrator never approved stays unreachable even
//!   for an owner.
//! - A schedule that targets a remote node keeps that target through
//!   creation, readback, and materialization.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::remote_targets::RemoteTargetPolicy;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::dto::{ExecutionTargetDto, JobQueryDto, JobSubmitDto, ScheduleCreateDto};
use codegg_config::schema::{EggworkConfig, EggworkNodeProfile};
use codegg_core::identity::ProjectId;
use codegg_core::jobs::schedule::{JobTemplate, MissedRunPolicy, OverlapPolicy, ScheduleKind};
use codegg_core::jobs::{
    ExecutionTarget, IdempotencyClass, JobKind, JobPayload, JobPriority, ResourceRequest,
    RetryPolicy,
};
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

use std::collections::HashMap;
use std::path::Path;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

/// A policy that approves exactly one node, `builder`.
fn approving_policy() -> RemoteTargetPolicy {
    let mut nodes = HashMap::new();
    nodes.insert(
        "builder".to_owned(),
        EggworkNodeProfile {
            node_id: Some("builder-1".to_owned()),
            endpoint: Some("https://node.invalid".to_owned()),
            ..Default::default()
        },
    );
    RemoteTargetPolicy::from_config(&EggworkConfig {
        nodes: Some(nodes),
        allowed_target_nodes: Some(vec!["builder".to_owned()]),
        ..Default::default()
    })
}

fn daemon_with_policy(pool: sqlx::SqlitePool, policy: RemoteTargetPolicy) -> CoreDaemon {
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool), None, None).with_remote_target_policy(policy),
    );
    // The local-owner client owns the daemon, so it passes every
    // authorization gate (including the node-target gate) without team
    // state — exactly as it does for every other operation.
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
    daemon
}

const LOCAL_CLIENT: &str = "client-local";

/// Register a workspace so a job submission has a workspace to bind to.
async fn register_workspace(daemon: &CoreDaemon, root: &Path) -> String {
    match call(
        daemon,
        LOCAL_CLIENT,
        CoreRequest::WorkspaceRegister {
            root: root.to_string_lossy().into_owned(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("expected workspace snapshot, got {other:?}"),
    }
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

/// A one-shot schedule spec whose job template carries the given
/// execution target.
fn schedule_spec(workspace_id: &str, job_template: JobTemplate) -> ScheduleCreateDto {
    let kind = ScheduleKind::OneShot {
        run_at: chrono::Utc::now() + chrono::Duration::seconds(60),
    };
    ScheduleCreateDto {
        workspace_id: workspace_id.to_owned(),
        session_id: None,
        kind: serde_json::to_value(kind).expect("schedule kind serializes"),
        job_template: serde_json::to_value(job_template).expect("job template serializes"),
        overlap_policy: serde_json::to_value(OverlapPolicy::SkipIfRunning)
            .expect("overlap policy serializes")
            .as_str()
            .expect("overlap policy is a string")
            .to_owned(),
        missed_run_policy: serde_json::to_value(MissedRunPolicy::RunOnceNow)
            .expect("missed-run policy serializes"),
        labels: std::collections::HashMap::new(),
    }
}

/// A remote-capable job template: a `build` job with a typed command.
fn remote_template(node_id: &str) -> JobTemplate {
    JobTemplate {
        kind: JobKind::Build,
        priority: JobPriority::Background,
        payload: JobPayload::ManagedArgv {
            argv: vec!["cargo".to_owned(), "build".to_owned()],
            cwd: None,
        },
        resource_request: ResourceRequest::for_kind(JobKind::Build),
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        timeout: None,
        labels: std::collections::HashMap::new(),
        target: ExecutionTarget::EggworkNode {
            node_id: node_id.to_owned(),
        },
    }
}

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

fn error_message(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { message, .. } => message,
        other => panic!("expected error response, got {other:?}"),
    }
}

fn build_payload() -> serde_json::Value {
    serde_json::json!({
        "kind": "managed_argv",
        "argv": ["cargo", "build", "--release"],
        "cwd": null,
        "env": null,
        "stdin": null,
    })
}

fn submit_spec(
    workspace_id: &str,
    session_id: Option<String>,
    kind: &str,
    target: Option<ExecutionTargetDto>,
) -> JobSubmitDto {
    let payload = match kind {
        "build" => build_payload(),
        "test" => serde_json::json!({
            "kind": "test",
            "command": "cargo test",
            "argv": ["cargo", "test"],
            "cwd": null,
            "scope": "workspace",
            "parent_run_id": null,
        }),
        _ => serde_json::json!({
            "kind": "subagent",
            "prompt": "do the thing",
            "agent": "build",
            "model": null,
            "parent_id": null,
            "denied_tools": [],
            "allowed_paths": [],
            "max_tool_calls": null,
        }),
    };
    JobSubmitDto {
        submission_key: None,
        workspace_id: workspace_id.to_owned(),
        session_id,
        turn_id: None,
        kind: kind.to_owned(),
        priority: "background".to_owned(),
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
        target,
    }
}

fn remote_target(node_id: &str) -> Option<ExecutionTargetDto> {
    Some(ExecutionTargetDto::EggworkNode {
        node_id: node_id.to_owned(),
    })
}

async fn job_count(daemon: &CoreDaemon) -> usize {
    match call(
        daemon,
        LOCAL_CLIENT,
        CoreRequest::JobList {
            query: JobQueryDto {
                workspace_id: None,
                states: Vec::new(),
                kinds: Vec::new(),
                session_id: None,
                limit: 100,
            },
        },
    )
    .await
    {
        CoreResponse::JobList { jobs } => jobs.len(),
        other => panic!("expected job list, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn local_submission_is_unchanged_by_the_remote_boundary() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", None),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "local submission must keep working, got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn approved_remote_target_is_admitted_for_a_remote_runnable_kind() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", remote_target("builder")),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "approved remote target must be admitted, got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn unapproved_node_is_refused_and_creates_no_job() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", remote_target("other-node")),
        },
    )
    .await;
    assert_eq!(error_code(&response), "remote_target_not_approved");
    assert_eq!(
        job_count(&daemon).await,
        0,
        "a refused remote submission must not create a job"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_without_approved_nodes_refuses_every_remote_target() {
    let pool = test_pool().await;
    // A daemon with configured nodes but no approved target list.
    let mut nodes = HashMap::new();
    nodes.insert(
        "builder".to_owned(),
        EggworkNodeProfile {
            endpoint: Some("https://node.invalid".to_owned()),
            ..Default::default()
        },
    );
    let daemon = daemon_with_policy(
        pool,
        RemoteTargetPolicy::from_config(&EggworkConfig {
            nodes: Some(nodes),
            ..Default::default()
        }),
    );
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", remote_target("builder")),
        },
    )
    .await;
    assert_eq!(error_code(&response), "remote_target_not_approved");
    assert_eq!(job_count(&daemon).await, 0);

    // Local submission keeps working on the same daemon.
    let local = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", None),
        },
    )
    .await;
    assert!(matches!(local, CoreResponse::JobSubmitted { .. }));
}

#[tokio::test(flavor = "current_thread")]
async fn remote_kind_without_a_remote_executor_is_refused() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    for (kind, expected) in [
        ("test", "remote_target_kind_deferred"),
        ("subagent", "remote_target_unsupported_kind"),
    ] {
        let response = call(
            &daemon,
            LOCAL_CLIENT,
            CoreRequest::JobSubmit {
                spec: submit_spec(&workspace_id, None, kind, remote_target("builder")),
            },
        )
        .await;
        assert_eq!(error_code(&response), expected, "kind {kind}");
        assert_eq!(
            job_count(&daemon).await,
            0,
            "kind {kind} must create no job"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_target_is_a_typed_refusal_not_a_local_fallback() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                None,
                "build",
                Some(ExecutionTargetDto::Invalid {
                    reason: "unknown execution target kind 'kubernetes_node'".to_owned(),
                }),
            ),
        },
    )
    .await;
    assert_eq!(error_code(&response), "invalid_job_submit");
    assert!(
        error_message(&response).contains("kubernetes_node"),
        "the refusal must name the reason: {}",
        error_message(&response)
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn schedule_target_is_preserved_and_refused_when_not_approved() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleCreate {
            spec: schedule_spec(&workspace_id, remote_template("builder")),
        },
    )
    .await;
    let schedule_id = match response {
        CoreResponse::ScheduleCreated { schedule_id } => schedule_id,
        other => panic!("expected schedule creation, got {other:?}"),
    };

    // The readback carries the target identity — and nothing else.
    let record = match call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleGet {
            schedule_id: schedule_id.clone(),
        },
    )
    .await
    {
        CoreResponse::ScheduleGet { schedule } => schedule,
        other => panic!("expected schedule record, got {other:?}"),
    };
    assert_eq!(record.target_node_id.as_deref(), Some("builder"));
    assert!(
        record.job_template["target"]["node_id"] == "builder",
        "the persisted template keeps the target: {}",
        record.job_template
    );
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(
        !serialized.contains("node.invalid"),
        "the readback must not carry an endpoint: {serialized}"
    );

    // A schedule that names an unapproved node never becomes durable.
    let refused = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleCreate {
            spec: schedule_spec(&workspace_id, remote_template("other-node")),
        },
    )
    .await;
    assert_eq!(error_code(&refused), "remote_target_not_approved");

    let schedules = match call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleList {
            workspace_id: None,
            include_archived: false,
        },
    )
    .await
    {
        CoreResponse::ScheduleList { schedules } => schedules,
        other => panic!("expected schedule list, got {other:?}"),
    };
    assert_eq!(schedules.len(), 1, "the refused schedule was not created");
}

#[tokio::test(flavor = "current_thread")]
async fn job_readback_carries_the_target_identity_only() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, approving_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, "build", remote_target("builder")),
        },
    )
    .await;
    let job_id = match response {
        CoreResponse::JobSubmitted { job_id } => job_id,
        other => panic!("expected job submission, got {other:?}"),
    };

    let record = match call(
        &daemon,
        "local",
        CoreRequest::JobGet {
            job_id: job_id.clone(),
        },
    )
    .await
    {
        CoreResponse::JobGet { job } => job.expect("job record"),
        other => panic!("expected job record, got {other:?}"),
    };
    assert_eq!(record.target_node_id.as_deref(), Some("builder"));
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(
        !serialized.contains("node.invalid"),
        "the readback must not carry an endpoint: {serialized}"
    );
}

// ── Principal gate ─────────────────────────────────────────────────────

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

/// Register a project bound to `workspace_id` and return its id.
async fn register_project(daemon: &CoreDaemon, workspace_id: &str, name: &str) -> ProjectId {
    match call(
        daemon,
        LOCAL_CLIENT,
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
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => {
            ProjectId::parse(&project.project_id).expect("catalog project id parses")
        }
        other => panic!("expected project registration, got {other:?}"),
    }
}

/// Create a session bound to `project` inside an already-registered
/// workspace.
async fn session_for_project(
    daemon: &CoreDaemon,
    client: &str,
    project_id: &str,
    workspace_id: &str,
    root: &Path,
) -> String {
    match call(
        daemon,
        client,
        CoreRequest::SessionCreate {
            directory: root.to_string_lossy().into_owned(),
            title: Some("target boundary".to_owned()),
            project_id: Some(project_id.to_owned()),
            workspace_id: Some(workspace_id.to_owned()),
        },
    )
    .await
    {
        CoreResponse::Session { session } => session.id,
        other => panic!("expected session, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn contributor_cannot_name_a_remote_node_but_owner_can() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool.clone(), approving_policy());
    let team = TeamStore::new(pool);
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;
    let project = register_project(&daemon, &workspace_id, "target-boundary").await;

    let contributor = member_in_project(
        &team,
        &project,
        "Contributor",
        "client-contributor",
        ProjectRole::Contributor,
    )
    .await;
    let owner =
        member_in_project(&team, &project, "Owner", "client-owner", ProjectRole::Owner).await;
    daemon.clients.register_with_principal(
        "client-contributor".to_owned(),
        "contributor".to_owned(),
        None,
        contributor,
    );
    daemon.clients.register_with_principal(
        "client-owner".to_owned(),
        "owner".to_owned(),
        None,
        owner,
    );

    let session_id = session_for_project(
        &daemon,
        "client-owner",
        project.as_str(),
        &workspace_id,
        root.path(),
    )
    .await;

    // A contributor may submit jobs, but naming a remote node needs the
    // node-target capability, which the contributor role does not carry.
    let refused = call(
        &daemon,
        "client-contributor",
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                Some(session_id.clone()),
                "build",
                remote_target("builder"),
            ),
        },
    )
    .await;
    assert_eq!(error_code(&refused), "authorization_denied");
    assert_eq!(job_count(&daemon).await, 0);

    // The project owner holds `node.target` and is admitted.
    let allowed = call(
        &daemon,
        "client-owner",
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                Some(session_id.clone()),
                "build",
                remote_target("builder"),
            ),
        },
    )
    .await;
    assert!(
        matches!(allowed, CoreResponse::JobSubmitted { .. }),
        "an owner may name an approved node, got {allowed:?}"
    );
    assert_eq!(job_count(&daemon).await, 1);
}
