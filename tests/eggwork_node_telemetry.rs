//! Eggwork M002 — authenticated host telemetry projection.
//!
//! Boundary proof for the plan: an operator may read the host telemetry of
//! a configured remote node, and what comes back is bounded, sanitized, and
//! honest about what it does not know.
//!
//! - A read reports the node's own state and the dimensions the node
//!   actually reported. A dimension the node could not measure is
//!   `unsupported`, never a zero.
//! - A node without a provider, a node that does not implement the
//!   telemetry contract, and a node that refuses the read produce
//!   *distinct* bounded diagnostics. None of them fabricates a value.
//! - The observation is joined to a node only after the node identity has
//!   been checked, so a document addressed to another node is refused.
//! - Two readers of the same node share one upstream read, and a repeated
//!   read inside the freshness window is served from the daemon's cache:
//!   a refresh loop cannot amplify the node's poll of its provider.
//! - The read is read-only. It is gated on the existing node-target grant,
//!   a denied reader never reaches the node, and a read never creates a job
//!   or touches the queue.

mod common;

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::dto::{NodeMetricStateDto, NodeTelemetryDto, NodeTelemetryStateDto};
use codegg::scheduler::eggwork::ResolvedEggworkNode;
use codegg::scheduler::eggwork_telemetry::NodeTelemetryService;
use codegg_core::identity::ProjectId;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

use common::eggwork_node::{ScriptedNodeClient, ScriptedNodeFactory, ScriptedTelemetryError};
use eggwork_core::{
    HostTelemetry, NodeId, NodeTelemetry, TelemetryCpu, TelemetryDisk, TelemetryDiskIo,
    TelemetryErrorCategory, TelemetryMemory, TelemetryMetric, TelemetryNetwork,
    TelemetryProviderState, TelemetrySource,
};

const LOCAL_CLIENT: &str = "client-local";
const NODE_KEY: &str = "builder";
const NODE_ID: &str = "builder-1";

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

/// The resolved node the coordinator reads through.
fn resolved_node() -> ResolvedEggworkNode {
    ResolvedEggworkNode {
        node_id: NODE_ID.to_owned(),
        endpoint: "https://node.invalid".to_owned(),
        ca_cert: PathBuf::from("/nonexistent/ca.pem"),
        client_cert: PathBuf::from("/nonexistent/client.pem"),
        client_key: PathBuf::from("/nonexistent/client.key"),
        required_capabilities: Vec::new(),
        isolation_policy: Default::default(),
        network_policy: Default::default(),
        resource_limits: None,
    }
}

/// One configured node.
fn node_table() -> HashMap<String, ResolvedEggworkNode> {
    let mut nodes = HashMap::new();
    nodes.insert(NODE_KEY.to_owned(), resolved_node());
    nodes
}

/// A daemon whose telemetry coordinator reads through `client`.
fn daemon_with_node(pool: sqlx::SqlitePool, client: Arc<ScriptedNodeClient>) -> CoreDaemon {
    let service = Arc::new(NodeTelemetryService::new(
        node_table(),
        Arc::new(ScriptedNodeFactory::new(client)),
    ));
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool), None, None).with_node_telemetry(service),
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

/// Read the telemetry of every configured node.
async fn read_nodes(daemon: &CoreDaemon, client: &str) -> Vec<NodeTelemetryDto> {
    match call(daemon, client, CoreRequest::NodeTelemetry { node_id: None }).await {
        CoreResponse::NodeTelemetry { nodes } => nodes,
        other => panic!("expected node telemetry, got {other:?}"),
    }
}

fn error_code(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code.clone(),
        other => panic!("expected an error response, got {other:?}"),
    }
}

/// A ready observation with one unsupported dimension and one unknown one.
fn ready_observation() -> HostTelemetry {
    HostTelemetry {
        observed_at_unix_ms: 1_700_000_000_000,
        sample_interval_ms: 2_000,
        cpu: TelemetryMetric::Value(TelemetryCpu {
            logical_cores: 8,
            usage_pct: 12.5,
            iowait_pct: None,
        }),
        memory: TelemetryMetric::Value(TelemetryMemory {
            available_bytes: 1_000,
            total_bytes: 2_000,
            used_bytes: 1_000,
        }),
        load: TelemetryMetric::Unsupported,
        swap: TelemetryMetric::Unknown,
        disk: TelemetryMetric::Value(TelemetryDisk {
            available_bytes: 10,
            total_bytes: 20,
            volume_count: 2,
        }),
        disk_io: TelemetryMetric::Value(TelemetryDiskIo {
            read_bytes_per_sec: 3,
            write_bytes_per_sec: 4,
        }),
        network: TelemetryMetric::Value(TelemetryNetwork {
            rx_bytes_per_sec: 5,
            tx_bytes_per_sec: 6,
        }),
    }
}

fn telemetry_document(
    state: TelemetryProviderState,
    observation: Option<HostTelemetry>,
) -> NodeTelemetry {
    NodeTelemetry {
        schema_version: 1,
        node_id: NodeId::new(NODE_ID).unwrap(),
        source: TelemetrySource::Gregg,
        state,
        age_ms: Some(1_500),
        last_error: None,
        observation,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_authorized_read_reports_the_node_state_and_only_what_the_node_reported() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    assert_eq!(node.node_id, NODE_KEY);
    assert_eq!(node.state, NodeTelemetryStateDto::Ready);
    assert_eq!(node.age_ms, Some(1_500));
    assert_eq!(node.observed_at_unix_ms, Some(1_700_000_000_000));
    assert_eq!(node.diagnostic, None);

    let cpu = node.cpu.expect("cpu reported");
    assert_eq!(cpu.state, NodeMetricStateDto::Value);
    assert_eq!(cpu.logical_cores, Some(8));
    assert_eq!(cpu.usage_pct, Some(12.5));
    // The host does not report iowait: absent, never zero.
    assert_eq!(cpu.iowait_pct, None);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unsupported_dimension_is_reported_as_unsupported_and_never_as_zero() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];

    let load = node.load.expect("load dimension present");
    assert_eq!(load.state, NodeMetricStateDto::Unsupported);
    assert_eq!(load.one, None);
    assert_eq!(load.five, None);
    assert_eq!(load.fifteen, None);

    let swap = node.swap.expect("swap dimension present");
    assert_eq!(swap.state, NodeMetricStateDto::Unknown);
    assert_eq!(swap.used_bytes, None);
    assert_eq!(swap.total_bytes, None);
}

#[tokio::test(flavor = "current_thread")]
async fn a_node_without_a_provider_reports_disabled_and_fabricates_nothing() {
    let pool = test_pool().await;
    let client = Arc::new(
        ScriptedNodeClient::new()
            .with_telemetry(telemetry_document(TelemetryProviderState::Disabled, None)),
    );
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];
    assert_eq!(node.state, NodeTelemetryStateDto::Disabled);
    assert!(node.cpu.is_none());
    assert!(node.memory.is_none());
    assert_eq!(node.observed_at_unix_ms, None);
    assert!(node.diagnostic.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn a_node_without_the_telemetry_contract_reports_a_distinct_diagnostic() {
    let pool = test_pool().await;
    // The fake answers "unsupported" when no document is scripted, which is
    // exactly what an old node does.
    let client = Arc::new(ScriptedNodeClient::new());
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];
    assert_eq!(node.state, NodeTelemetryStateDto::Unavailable);
    let diagnostic = node.diagnostic.as_deref().expect("a diagnostic");
    assert!(
        diagnostic.contains("does not implement"),
        "expected an unsupported-contract diagnostic, got {diagnostic}"
    );
    assert!(node.cpu.is_none());
    assert!(node.memory.is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn a_stale_observation_is_reported_as_stale_and_keeps_its_age() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Stale,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];
    assert_eq!(node.state, NodeTelemetryStateDto::Stale);
    assert_eq!(node.age_ms, Some(1_500));
    // The numbers the node did report are still carried.
    assert!(node.cpu.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn the_provider_error_category_is_carried_without_its_message() {
    let pool = test_pool().await;
    let mut document = telemetry_document(TelemetryProviderState::Unavailable, None);
    document.last_error = Some(TelemetryErrorCategory::Timeout);
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(document));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    assert_eq!(nodes[0].last_error.as_deref(), Some("timeout"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_document_addressed_to_another_node_is_refused() {
    let pool = test_pool().await;
    let mut document = telemetry_document(TelemetryProviderState::Ready, Some(ready_observation()));
    document.node_id = NodeId::new("someone-else").unwrap();
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(document));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];
    // The observation is never joined to the requested node.
    assert_eq!(node.node_id, NODE_KEY);
    assert!(node.cpu.is_none());
    assert!(node.diagnostic.is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn two_readers_of_the_same_node_share_one_upstream_read() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client.clone());

    // Two readers, concurrently, of the same node.
    let first = read_nodes(&daemon, LOCAL_CLIENT);
    let second = read_nodes(&daemon, LOCAL_CLIENT);
    let (first, second) = tokio::join!(first, second);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);

    // The node served one read, not two: the coordinator coalesced the
    // readers onto a single upstream read.
    assert_eq!(
        client.telemetry_reads(),
        1,
        "concurrent readers must not amplify the node's poll"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_repeated_read_inside_the_freshness_window_is_served_from_the_cache() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client.clone());

    read_nodes(&daemon, LOCAL_CLIENT).await;
    read_nodes(&daemon, LOCAL_CLIENT).await;
    read_nodes(&daemon, LOCAL_CLIENT).await;

    assert_eq!(
        client.telemetry_reads(),
        1,
        "a refresh loop must not amplify the node's poll"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_unknown_node_is_a_typed_refusal() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new());
    let daemon = daemon_with_node(pool, client.clone());

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::NodeTelemetry {
            node_id: Some("nope".to_owned()),
        },
    )
    .await;
    assert_eq!(error_code(&response), "unknown_eggwork_node");
    assert_eq!(client.telemetry_reads(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_daemon_without_nodes_reports_no_nodes_configured() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::with_deps(CoreRuntimeDeps::new(Some(pool), None, None));
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );

    let response = call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::NodeTelemetry { node_id: None },
    )
    .await;
    assert_eq!(error_code(&response), "node_telemetry_unavailable");
}

#[tokio::test(flavor = "current_thread")]
async fn a_reader_without_the_node_grant_is_denied_and_never_reaches_the_node() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool.clone(), client.clone());

    // A contributor holds neither the node-target capability nor any
    // project scope, so the read is refused before any network call.
    let team = TeamStore::new(pool);
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_id = register_workspace(&daemon, root.path()).await;
    let project = register_project(&daemon, &workspace_id, "telemetry-boundary").await;
    let contributor = member_in_project(
        &team,
        &project,
        "Contributor",
        "client-contributor",
        ProjectRole::Contributor,
    )
    .await;
    daemon.clients.register_with_principal(
        "client-contributor".to_owned(),
        "contributor".to_owned(),
        None,
        contributor,
    );

    let response = call(
        &daemon,
        "client-contributor",
        CoreRequest::NodeTelemetry { node_id: None },
    )
    .await;
    // The read is scoped to no project, so a team principal fails closed on
    // the missing scope before the capability is even consulted. Only the
    // local owner's broad policy authorizes a daemon-level read, and the
    // refusal happens before any network call.
    assert_eq!(error_code(&response), "authorization_scope_required");
    assert_eq!(
        client.telemetry_reads(),
        0,
        "a denied reader must never reach the node"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_telemetry_read_creates_no_job_and_leaves_the_queue_alone() {
    let pool = test_pool().await;
    let client = Arc::new(ScriptedNodeClient::new().with_telemetry(telemetry_document(
        TelemetryProviderState::Ready,
        Some(ready_observation()),
    )));
    let daemon = daemon_with_node(pool, client);

    let before = job_count(&daemon).await;
    read_nodes(&daemon, LOCAL_CLIENT).await;
    read_nodes(&daemon, LOCAL_CLIENT).await;
    assert_eq!(job_count(&daemon).await, before);
}

#[tokio::test(flavor = "current_thread")]
async fn a_failed_probe_reports_a_diagnostic_and_never_a_measurement() {
    let pool = test_pool().await;
    let client =
        Arc::new(ScriptedNodeClient::new().with_telemetry_error(ScriptedTelemetryError::Transport));
    let daemon = daemon_with_node(pool, client);

    let nodes = read_nodes(&daemon, LOCAL_CLIENT).await;
    let node = &nodes[0];
    assert_eq!(node.state, NodeTelemetryStateDto::Unavailable);
    assert!(node.cpu.is_none());
    assert!(node.memory.is_none());
    assert!(node.diagnostic.is_some());
}

/// Register a workspace so a project can be bound to one.
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

/// Register a project so a team principal can hold a project role.
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

/// Mint a human principal for `client_id`.
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

/// Create a team principal holding `role` in `project`.
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

/// How many jobs the daemon knows about.
async fn job_count(daemon: &CoreDaemon) -> usize {
    match call(
        daemon,
        LOCAL_CLIENT,
        CoreRequest::JobList {
            query: codegg::protocol::dto::JobQueryDto::default(),
        },
    )
    .await
    {
        CoreResponse::JobList { jobs, .. } => jobs.len(),
        other => panic!("expected a job list, got {other:?}"),
    }
}
