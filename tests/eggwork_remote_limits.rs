//! M003 — remote resource admission and limits.
//!
//! Boundary proof for the plan's work packages A and D:
//!
//! - The operator's configured ceiling is authoritative. A job may narrow
//!   a limit; it may never widen one, never introduce a dimension the
//!   operator never configured, and never weaken a mandatory limit.
//! - Every refusal happens at the host boundary, before the job becomes
//!   durable, so a refused submission creates no job at all.
//! - The abstract scheduler tokens (`cpu_weight`, `memory_mb_hint`) and
//!   the physical remote limits are different vocabularies. A client that
//!   puts a scheduler token in the remote slot is refused, not silently
//!   reinterpreted.
//! - A job that asks for no limits keeps the pre-M003 behavior exactly.
//! - Local execution is untouched by every remote gate.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::remote_targets::RemoteTargetPolicy;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::dto::{
    ExecutionTargetDto, JobQueryDto, JobSubmitDto, RemoteLimitEnforcementDto,
    RemoteResourceLimitDto, RemoteResourceLimitsDto,
};
use codegg_config::schema::{
    EggworkConfig, EggworkNodeProfile, RemoteLimitEnforcement, RemoteResourceLimit,
    RemoteResourceLimits,
};
use codegg_core::transport_auth::AuthenticatedPrincipal;

use std::collections::HashMap;
use std::path::Path;

const LOCAL_CLIENT: &str = "client-local";

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn limit(value: u64, enforcement: RemoteLimitEnforcement) -> RemoteResourceLimit {
    RemoteResourceLimit { value, enforcement }
}

/// A policy that approves one node, `builder`, with an operator ceiling.
///
/// A memory ceiling of 1 GiB, a CPU quota of 1000 milli-percent (one
/// full CPU), and a PID ceiling of 64, all mandatory.
fn ceiling_policy() -> RemoteTargetPolicy {
    let mut nodes = HashMap::new();
    nodes.insert(
        "builder".to_owned(),
        EggworkNodeProfile {
            node_id: Some("builder-1".to_owned()),
            endpoint: Some("https://node.invalid".to_owned()),
            resource_limits: Some(RemoteResourceLimits {
                memory_bytes: Some(limit(1024 * 1024 * 1024, RemoteLimitEnforcement::Required)),
                cpu_quota_milli_percent: Some(limit(1000, RemoteLimitEnforcement::Required)),
                pids: Some(limit(64, RemoteLimitEnforcement::Required)),
            }),
            ..Default::default()
        },
    );
    RemoteTargetPolicy::from_config(&EggworkConfig {
        nodes: Some(nodes),
        allowed_target_nodes: Some(vec!["builder".to_owned()]),
        ..Default::default()
    })
}

/// A policy that approves the node but configures no resource ceiling.
fn limitless_policy() -> RemoteTargetPolicy {
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
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
    daemon
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

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
    target: Option<ExecutionTargetDto>,
    remote_resources: Option<RemoteResourceLimitsDto>,
) -> JobSubmitDto {
    JobSubmitDto {
        submission_key: None,
        workspace_id: workspace_id.to_owned(),
        session_id: None,
        turn_id: None,
        kind: "build".to_owned(),
        priority: "background".to_owned(),
        source: serde_json::json!({"kind": "interactive"}),
        payload: build_payload(),
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
        remote_resources,
    }
}

fn remote_target(node_id: &str) -> Option<ExecutionTargetDto> {
    Some(ExecutionTargetDto::EggworkNode {
        node_id: node_id.to_owned(),
    })
}

fn dto_limit(value: u64, enforcement: RemoteLimitEnforcementDto) -> RemoteResourceLimitDto {
    RemoteResourceLimitDto { value, enforcement }
}

fn dto_limits(
    memory_bytes: Option<RemoteResourceLimitDto>,
    cpu_quota_milli_percent: Option<RemoteResourceLimitDto>,
    pids: Option<RemoteResourceLimitDto>,
) -> RemoteResourceLimitsDto {
    RemoteResourceLimitsDto::Limits {
        memory_bytes,
        cpu_quota_milli_percent,
        pids,
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

// ── The legacy no-limit profile is unchanged ────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_remote_job_without_limits_is_admitted_and_keeps_the_legacy_profile() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), None),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "a remote job with no limits must still be admitted, got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn the_durable_job_carries_no_remote_limits_when_none_were_requested() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), None),
        },
    )
    .await;
    let job_id = match response {
        CoreResponse::JobSubmitted { job_id } => job_id,
        other => panic!("expected job submission, got {other:?}"),
    };
    let record = match call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobGet {
            job_id: job_id.clone(),
        },
    )
    .await
    {
        CoreResponse::JobGet { job } => job.expect("job record"),
        other => panic!("expected job record, got {other:?}"),
    };
    // The readback carries the target identity only — never a limit value
    // that an operator did not write.
    let serialized = serde_json::to_string(&record).unwrap();
    assert!(
        !serialized.contains("cpu_quota_milli_percent"),
        "{serialized}"
    );
    assert!(!serialized.contains("node.invalid"), "{serialized}");
}

#[tokio::test(flavor = "current_thread")]
async fn local_submission_is_untouched_by_the_remote_limit_gates() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, None, None),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "local submission must keep working, got {response:?}"
    );
}

// ── The operator ceiling is authoritative ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_narrowing_request_is_admitted() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    Some(dto_limit(
                        512 * 1024 * 1024,
                        RemoteLimitEnforcementDto::Required,
                    )),
                    Some(dto_limit(500, RemoteLimitEnforcementDto::Required)),
                    Some(dto_limit(32, RemoteLimitEnforcementDto::Required)),
                )),
            ),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "narrowing the ceiling is allowed, got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_above_the_ceiling_is_refused_and_creates_no_job() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    Some(dto_limit(
                        2 * 1024 * 1024 * 1024,
                        RemoteLimitEnforcementDto::Required,
                    )),
                    None,
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_exceeds_ceiling",
        "got {response:?}"
    );
    assert_eq!(
        job_count(&daemon).await,
        0,
        "a refused request leaves no job"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_that_weakens_a_mandatory_limit_is_refused() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    None,
                    Some(dto_limit(500, RemoteLimitEnforcementDto::BestEffort)),
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_downgrade_refused",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_for_an_unconfigured_dimension_is_refused() {
    let pool = test_pool().await;
    let _daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let _workspace_id = register_workspace(&_daemon, root.path()).await;

    // The ceiling configures memory, CPU, and PIDs — but a *second* node
    // policy with only memory configured refuses a PIDs request.
    let mut nodes = HashMap::new();
    nodes.insert(
        "builder".to_owned(),
        EggworkNodeProfile {
            node_id: Some("builder-1".to_owned()),
            endpoint: Some("https://node.invalid".to_owned()),
            resource_limits: Some(RemoteResourceLimits {
                memory_bytes: Some(limit(1024 * 1024 * 1024, RemoteLimitEnforcement::Required)),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let policy = RemoteTargetPolicy::from_config(&EggworkConfig {
        nodes: Some(nodes),
        allowed_target_nodes: Some(vec!["builder".to_owned()]),
        ..Default::default()
    });
    let daemon = daemon_with_policy(test_pool().await, policy);
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    None,
                    None,
                    Some(dto_limit(8, RemoteLimitEnforcementDto::Required)),
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_not_configured",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_against_a_node_without_a_ceiling_is_refused() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, limitless_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    Some(dto_limit(1024, RemoteLimitEnforcementDto::Required)),
                    None,
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_not_configured",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_remote_job_without_limits_is_admitted_against_a_ceilingless_node() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, limitless_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), None),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "a ceiling-less node still runs unlimited remote work, got {response:?}"
    );
}

// ── Units, overflow, and token confusion fail closed ────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_zero_limit_is_refused_rather_than_read_as_unlimited() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    Some(dto_limit(0, RemoteLimitEnforcementDto::Required)),
                    None,
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_invalid_value",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_limit_beyond_the_node_bound_is_refused_rather_than_clamped() {
    // A ceiling that is itself beyond the node's bound (which the
    // configuration loader would have rejected) proves the request is
    // refused by the *node's* bound, not merely by the ceiling.
    let mut nodes = HashMap::new();
    nodes.insert(
        "builder".to_owned(),
        EggworkNodeProfile {
            node_id: Some("builder-1".to_owned()),
            endpoint: Some("https://node.invalid".to_owned()),
            resource_limits: Some(RemoteResourceLimits {
                cpu_quota_milli_percent: Some(limit(2_000_000, RemoteLimitEnforcement::Required)),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let policy = RemoteTargetPolicy::from_config(&EggworkConfig {
        nodes: Some(nodes),
        allowed_target_nodes: Some(vec!["builder".to_owned()]),
        ..Default::default()
    });
    let daemon = daemon_with_policy(test_pool().await, policy);
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    None,
                    Some(dto_limit(1_500_000, RemoteLimitEnforcementDto::Required)),
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_invalid_value",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_request_above_the_ceiling_is_refused_before_the_node_bound_is_consulted() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    // The ceiling is 1000 milli-percent; 1000001 is both above the
    // ceiling and above the node's bound. The ceiling is the operator's
    // authority, so the ceiling is what the refusal names.
    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    None,
                    Some(dto_limit(1_000_001, RemoteLimitEnforcementDto::Required)),
                    None,
                )),
            ),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_exceeds_ceiling",
        "got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn an_abstract_scheduler_token_in_the_remote_slot_is_refused() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    // `cpu_weight` is a scheduler token, not a remote limit. Decoding it
    // as a limit set must fail closed rather than ignore the key and run
    // the job unconstrained.
    let limits: RemoteResourceLimitsDto = serde_json::from_value(serde_json::json!({
        "cpu_weight": 3
    }))
    .expect("the wire form decodes");
    assert!(
        matches!(limits, RemoteResourceLimitsDto::Invalid { .. }),
        "a scheduler token must not decode as a remote limit"
    );

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), Some(limits)),
        },
    )
    .await;
    assert!(
        !matches!(response, CoreResponse::JobSubmitted { .. }),
        "a confused limit must not be admitted, got {response:?}"
    );
    assert_eq!(job_count(&daemon).await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_credential_bearing_locator_cannot_ride_along_in_a_limit() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let limits: RemoteResourceLimitsDto = serde_json::from_value(serde_json::json!({
        "memory_bytes": {"value": 1024, "endpoint": "https://node.invalid"}
    }))
    .expect("the wire form decodes");
    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), Some(limits)),
        },
    )
    .await;
    assert!(
        !matches!(response, CoreResponse::JobSubmitted { .. }),
        "a limit carrying a locator must not be admitted, got {response:?}"
    );
    let message = error_message(&response);
    assert!(!message.contains("node.invalid"), "{message}");
    assert_eq!(job_count(&daemon).await, 0);
}

// ── The refusal message never leaks the operator's configuration ────────

#[tokio::test(flavor = "current_thread")]
async fn a_refusal_message_carries_no_endpoint_or_key_path() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(
                &workspace_id,
                remote_target("builder"),
                Some(dto_limits(
                    Some(dto_limit(
                        8 * 1024 * 1024 * 1024,
                        RemoteLimitEnforcementDto::Required,
                    )),
                    None,
                    None,
                )),
            ),
        },
    )
    .await;
    let message = error_message(&response);
    assert!(!message.contains("node.invalid"), "{message}");
    assert!(!message.contains("https://"), "{message}");
    assert!(!message.contains(".pem"), "{message}");
}

// ── The schedule path applies the same ceiling ──────────────────────────

fn remote_template(node_id: &str) -> codegg_core::jobs::schedule::JobTemplate {
    use codegg_core::jobs::schedule::JobTemplate;
    use codegg_core::jobs::{IdempotencyClass, JobKind, JobPayload, JobPriority, RetryPolicy};
    JobTemplate {
        kind: JobKind::Build,
        priority: JobPriority::Background,
        payload: JobPayload::ManagedArgv {
            argv: vec!["cargo".to_owned(), "build".to_owned()],
            cwd: None,
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(JobKind::Build),
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        timeout: None,
        labels: std::collections::HashMap::new(),
        target: codegg_core::jobs::ExecutionTarget::EggworkNode {
            node_id: node_id.to_owned(),
        },
    }
}

fn schedule_spec(
    workspace_id: &str,
    job_template: codegg_core::jobs::schedule::JobTemplate,
) -> codegg_protocol::dto::ScheduleCreateDto {
    use codegg_core::jobs::schedule::{MissedRunPolicy, OverlapPolicy, ScheduleKind};
    let kind = ScheduleKind::OneShot {
        run_at: chrono::Utc::now() + chrono::Duration::seconds(60),
    };
    codegg_protocol::dto::ScheduleCreateDto {
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

#[tokio::test(flavor = "current_thread")]
async fn a_schedule_whose_template_exceeds_the_ceiling_is_refused() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let mut template = remote_template("builder");
    template.resource_request.remote = Some(RemoteResourceLimits {
        memory_bytes: Some(limit(
            8 * 1024 * 1024 * 1024,
            RemoteLimitEnforcement::Required,
        )),
        cpu_quota_milli_percent: None,
        pids: None,
    });

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleCreate {
            spec: schedule_spec(&workspace_id, template),
        },
    )
    .await;
    assert_eq!(
        error_code(&response),
        "remote_limit_exceeds_ceiling",
        "got {response:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_schedule_within_the_ceiling_is_admitted() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    let mut template = remote_template("builder");
    template.resource_request.remote = Some(RemoteResourceLimits {
        memory_bytes: Some(limit(256 * 1024 * 1024, RemoteLimitEnforcement::Required)),
        cpu_quota_milli_percent: None,
        pids: None,
    });

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::ScheduleCreate {
            spec: schedule_spec(&workspace_id, template),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::ScheduleCreated { .. }),
        "a schedule within the ceiling must be admitted, got {response:?}"
    );
}

// ── The wire path ───────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn a_wire_encoded_limit_set_survives_the_round_trip_to_the_daemon() {
    let pool = test_pool().await;
    let daemon = daemon_with_policy(pool, ceiling_policy());
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;

    // Exactly what a client puts on the wire: one dimension, the other two
    // explicitly null.
    let wire = serde_json::json!({
        "kind": "limits",
        "memory_bytes": {"value": 256u64, "enforcement": "required"},
        "cpu_quota_milli_percent": null,
        "pids": null
    });
    let limits: RemoteResourceLimitsDto =
        serde_json::from_value(wire).expect("the wire form decodes");
    let RemoteResourceLimitsDto::Limits {
        cpu_quota_milli_percent,
        pids,
        ..
    } = &limits
    else {
        panic!("expected a decoded limit set")
    };
    assert_eq!(
        *cpu_quota_milli_percent, None,
        "a null dimension is absent, not a zero limit"
    );
    assert_eq!(*pids, None);

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobSubmit {
            spec: submit_spec(&workspace_id, remote_target("builder"), Some(limits)),
        },
    )
    .await;
    assert!(
        matches!(response, CoreResponse::JobSubmitted { .. }),
        "a request that omits a dimension must not be refused, got {response:?}"
    );
}
