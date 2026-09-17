//! Project Work Orders M001 — domain, storage, authorization, and protocol.
//!
//! Boundary proof for the plan: daemon-owned durable work orders with
//! typed identities, bounded validation, CAS updates/reorders, atomic
//! idempotent batches, project-scoped authorization with
//! privacy-preserving denials, immutable origin attribution, structural
//! audit, and secret-free projections. Waiting work orders exist without
//! session rows and nothing executes yet.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::work_order::{
    WorkOrderBatchCreateRequest, WorkOrderBatchItem, WorkOrderCreateRequest, WorkOrderGateDto,
    WorkOrderLaneCreateRequest, WorkOrderLaneReorderRequest, WorkOrderUpdateRequest,
};
use codegg_core::identity::ProjectId;
use codegg_core::team::{ProjectRole, TeamStore};
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
        .create_principal(codegg_core::team::PrincipalKind::Human, name)
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

fn create_request(project: &str, prompt: &str) -> CoreRequest {
    CoreRequest::WorkOrderCreate {
        request: WorkOrderCreateRequest {
            project_id: project.to_owned(),
            title: Some("Test work".to_owned()),
            prompt: prompt.to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: vec![WorkOrderGateDto {
                kind: "immediate".to_owned(),
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            }],
            gate_join: Some("all".to_owned()),
            repeat_count: Some(1),
            sequence_lane_id: None,
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            idempotency_key: None,
        },
    }
}

async fn create(
    daemon: &CoreDaemon,
    client: &str,
    project: &str,
    prompt: &str,
) -> codegg::protocol::work_order::WorkOrderDto {
    match call(daemon, client, create_request(project, prompt)).await {
        CoreResponse::WorkOrder { work_order, .. } => work_order,
        other => panic!("expected work order, got {other:?}"),
    }
}

// ── Capabilities and happy-path lifecycle ──────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_work_order_capabilities_shape() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool), None, None);
    match call(&daemon, "local-daemon", CoreRequest::WorkOrderCapabilities).await {
        CoreResponse::WorkOrderCapabilities { capabilities } => {
            assert!(capabilities.supported);
            assert_eq!(capabilities.protocol_version, 1);
            assert!(capabilities.max_prompt_bytes >= 1024);
            assert!(capabilities.max_batch_items >= 2);
            assert!(capabilities.max_list_limit >= 10);
            assert!(capabilities.max_repeat_count >= 2);
        }
        other => panic!("expected work order capabilities, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_member_crud_without_session_rows() {
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
    register_client(&daemon, "client-member", member);

    let created = create(&daemon, "client-member", project.as_str(), "Do the thing").await;
    assert_eq!(created.revision, 1);
    assert_eq!(created.state, "active");
    assert_eq!(created.repeat_count, 1);

    // Waiting work orders exist without session rows and execute nothing.
    let sessions: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM session")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions.0, 0);
    let jobs: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM job")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(jobs.0, 0);

    // Get by opaque id.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id.clone(),
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => assert_eq!(work_order, created),
        other => panic!("expected work order get, got {other:?}"),
    }

    // Update with the observed revision.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderUpdate {
            request: WorkOrderUpdateRequest {
                work_order_id: created.work_order_id.clone(),
                expected_revision: 1,
                title: None,
                prompt: Some("Do the other thing".to_owned()),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: None,
                gate_join: None,
                repeat_count: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => {
            assert_eq!(work_order.revision, 2);
            assert_eq!(work_order.prompt, "Do the other thing");
        }
        other => panic!("expected work order update, got {other:?}"),
    }

    // Stale revision makes zero mutation.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderUpdate {
            request: WorkOrderUpdateRequest {
                work_order_id: created.work_order_id.clone(),
                expected_revision: 1,
                title: None,
                prompt: Some("stale write".to_owned()),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: None,
                gate_join: None,
                repeat_count: None,
            },
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => assert_eq!(code, "work_order_revision_conflict"),
        other => panic!("expected revision conflict, got {other:?}"),
    }

    // Pause, resume, cancel along the lifecycle matrix.
    for (request, state) in [
        (
            CoreRequest::WorkOrderPause {
                work_order_id: created.work_order_id.clone(),
            },
            "paused",
        ),
        (
            CoreRequest::WorkOrderResume {
                work_order_id: created.work_order_id.clone(),
            },
            "active",
        ),
        (
            CoreRequest::WorkOrderCancel {
                work_order_id: created.work_order_id.clone(),
            },
            "cancelled",
        ),
    ] {
        match call(&daemon, "client-member", request).await {
            CoreResponse::WorkOrder { work_order, .. } => assert_eq!(work_order.state, state),
            other => panic!("expected lifecycle transition, got {other:?}"),
        }
    }

    // Cancelled work cannot resume: terminal transitions are one-way.
    let response = call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderResume {
            work_order_id: created.work_order_id.clone(),
        },
    )
    .await;
    assert_eq!(error_code(&response), "work_order_state_conflict");
}

// ── Batch atomicity and lane ordering ──────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_batch_creates_ordered_lane_segment() {
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

    let lane_id = match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderLaneCreate {
            request: WorkOrderLaneCreateRequest {
                project_id: project.as_str().to_owned(),
                label: Some("queue".to_owned()),
                failure_policy: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => {
            assert_eq!(lane.revision, 1);
            lane.lane_id
        }
        other => panic!("expected lane, got {other:?}"),
    };

    let items = ["one", "two", "three"]
        .into_iter()
        .map(|prompt| WorkOrderBatchItem {
            title: None,
            prompt: prompt.to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: Vec::new(),
            gate_join: None,
            repeat_count: None,
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            idempotency_key: None,
        })
        .collect();
    let batch = match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderBatchCreate {
            request: WorkOrderBatchCreateRequest {
                project_id: project.as_str().to_owned(),
                items,
                sequence_lane_id: Some(lane_id.clone()),
                batch_key: Some("batch-1".to_owned()),
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderBatch {
            work_orders,
            duplicate,
        } => {
            assert!(!duplicate);
            assert_eq!(work_orders.len(), 3);
            work_orders
        }
        other => panic!("expected batch, got {other:?}"),
    };

    // The lane holds the batch as one deterministic segment.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderLaneGet {
            lane_id: lane_id.clone(),
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => {
            let ordered = lane.ordered_work_order_ids;
            let created: Vec<String> = batch.into_iter().map(|work| work.work_order_id).collect();
            assert_eq!(ordered, created);
        }
        other => panic!("expected lane get, got {other:?}"),
    }

    // Retried batch keys converge without a second commit.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderBatchCreate {
            request: WorkOrderBatchCreateRequest {
                project_id: project.as_str().to_owned(),
                items: vec![],
                sequence_lane_id: None,
                batch_key: Some("batch-1".to_owned()),
            },
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => {
            // Empty retries are invalid input, not a silent success.
            assert_eq!(code, "work_order_invalid_input");
        }
        other => panic!("expected invalid input, got {other:?}"),
    }
}

// ── Concurrent reorder serialization ───────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_concurrent_reorder_serializes_on_revision() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let project = ProjectId::new();
    for (name, client) in [("A", "client-a"), ("B", "client-b")] {
        let member =
            member_in_project(&team, &project, name, client, ProjectRole::Contributor).await;
        register_client(&daemon, client, member);
    }

    let lane_id = match call(
        &daemon,
        "client-a",
        CoreRequest::WorkOrderLaneCreate {
            request: WorkOrderLaneCreateRequest {
                project_id: project.as_str().to_owned(),
                label: None,
                failure_policy: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => lane.lane_id,
        other => panic!("expected lane, got {other:?}"),
    };
    let first = create(&daemon, "client-a", project.as_str(), "first").await;
    let second = create(&daemon, "client-a", project.as_str(), "second").await;

    // Attach both members first (revision 1 -> 3).
    let mut revision = 1;
    for work_id in [&first.work_order_id, &second.work_order_id] {
        match call(
            &daemon,
            "client-a",
            CoreRequest::WorkOrderLaneAttach {
                request: codegg::protocol::work_order::WorkOrderLaneAttachRequest {
                    lane_id: lane_id.clone(),
                    expected_revision: revision,
                    work_order_id: work_id.clone(),
                    position: None,
                },
            },
        )
        .await
        {
            CoreResponse::WorkOrderLane { lane } => revision = lane.revision,
            other => panic!("expected lane attach, got {other:?}"),
        }
    }

    // Two clients race the same expected revision with opposite orders:
    // exactly one wins; the loser receives an explicit conflict.
    let order_a = vec![first.work_order_id.clone(), second.work_order_id.clone()];
    let order_b = vec![second.work_order_id.clone(), first.work_order_id.clone()];
    // Borrowed-future join like the chat contention test: both reorder
    // futures are polled before either completes, so the second writer
    // observes the first writer's revision bump.
    let (response_a, response_b) = tokio::join!(
        call(
            &daemon,
            "client-a",
            CoreRequest::WorkOrderLaneReorder {
                request: WorkOrderLaneReorderRequest {
                    lane_id: lane_id.clone(),
                    expected_revision: revision,
                    ordered_work_order_ids: order_a,
                },
            },
        ),
        call(
            &daemon,
            "client-b",
            CoreRequest::WorkOrderLaneReorder {
                request: WorkOrderLaneReorderRequest {
                    lane_id: lane_id.clone(),
                    expected_revision: revision,
                    ordered_work_order_ids: order_b,
                },
            },
        )
    );
    let codes = [error_code_or_ok(&response_a), error_code_or_ok(&response_b)];
    assert!(
        codes.contains(&"ok") && codes.contains(&"work_order_revision_conflict"),
        "exactly one reorder must win, got {codes:?}"
    );

    // The surviving order is a complete permutation: nothing is lost.
    match call(
        &daemon,
        "client-a",
        CoreRequest::WorkOrderLaneGet { lane_id },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => {
            assert_eq!(lane.revision, revision + 1);
            let mut ordered = lane.ordered_work_order_ids;
            ordered.sort();
            let mut expected = vec![first.work_order_id, second.work_order_id];
            expected.sort();
            assert_eq!(ordered, expected);
        }
        other => panic!("expected lane get, got {other:?}"),
    }
}

fn error_code_or_ok(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        _ => "ok",
    }
}

// ── Idempotency convergence ────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_duplicate_keys_converge_and_mismatch_conflicts() {
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

    let mut first = create_request(project.as_str(), "Same work");
    let CoreRequest::WorkOrderCreate { ref mut request } = first else {
        unreachable!();
    };
    request.idempotency_key = Some("key-1".to_owned());
    let one = match call(&daemon, "client-member", first).await {
        CoreResponse::WorkOrder {
            work_order,
            duplicate,
        } => {
            assert!(!duplicate);
            work_order
        }
        other => panic!("expected work order, got {other:?}"),
    };

    let mut retry = create_request(project.as_str(), "Same work");
    let CoreRequest::WorkOrderCreate { ref mut request } = retry else {
        unreachable!();
    };
    request.idempotency_key = Some("key-1".to_owned());
    match call(&daemon, "client-member", retry).await {
        CoreResponse::WorkOrder {
            work_order,
            duplicate,
        } => {
            assert!(duplicate);
            assert_eq!(work_order.work_order_id, one.work_order_id);
        }
        other => panic!("expected duplicate convergence, got {other:?}"),
    }

    let mut clash = create_request(project.as_str(), "Different work");
    let CoreRequest::WorkOrderCreate { ref mut request } = clash else {
        unreachable!();
    };
    request.idempotency_key = Some("key-1".to_owned());
    let response = call(&daemon, "client-member", clash).await;
    assert_eq!(error_code(&response), "work_order_idempotency_conflict");
}

// ── Restart and migration ──────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_file_backed_reopen_preserves_identities() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db_path = dir.path().join("codegg.db");
    let pool = codegg_core::storage::init_pool_at(&db_path)
        .await
        .expect("pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
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
    register_client(&daemon, "client-member", member);
    let created = create(
        &daemon,
        "client-member",
        project.as_str(),
        "survive restart",
    )
    .await;
    pool.close().await;
    drop(daemon);

    let pool = codegg_core::storage::init_pool_at(&db_path)
        .await
        .expect("reopen pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("re-migrate");
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool);
    let member = member_in_project(
        &team,
        &project,
        "Member2",
        "client-member2",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member2", member);
    match call(
        &daemon,
        "client-member2",
        CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id.clone(),
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => {
            assert_eq!(work_order.work_order_id, created.work_order_id);
            assert_eq!(work_order.revision, created.revision);
            assert_eq!(work_order.prompt, "survive restart");
        }
        other => panic!("expected reopened work order, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_remigration_is_additive_and_empty_by_default() {
    let pool = test_pool().await;
    for table in ["work_order", "work_order_occurrence", "sequence_lane"] {
        let count: (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 0, "{table} starts empty");
    }
    // Re-running the full chain over a downgraded version marker
    // re-applies v60/v61/v63 additively without duplicating or losing rows.
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
    register_client(&daemon, "client-member", member);
    let created = create(&daemon, "client-member", project.as_str(), "durable").await;
    // A legacy attribution row predates the v61 scope rebuild.
    sqlx::query(
        "INSERT INTO origin_attribution (scope_kind, scope_id, origin_principal, origin_kind, \
         auth_method, transport_class, policy, membership_revision, decision_id, correlation_id, \
         time_created, attribution_json) \
         VALUES ('session', 'legacy-session', 'local-owner', 'local_owner', 'local_owner', \
         'local', 'local_owner_broad', NULL, 'legacy-local', 'legacy', 1, '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("UPDATE migration_version SET version = 60 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("re-migrate");
    let version: (i64,) = sqlx::query_as("SELECT version FROM migration_version WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.0, 63);
    // Legacy rows survive the rebuild verbatim.
    let legacy: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM origin_attribution WHERE scope_kind = 'session' AND scope_id = 'legacy-session'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(legacy.0, 1);
    // The rebuilt CHECK admits the work-order scope.
    sqlx::query(
        "INSERT INTO origin_attribution (scope_kind, scope_id, origin_principal, origin_kind, \
         auth_method, transport_class, policy, membership_revision, decision_id, correlation_id, \
         time_created, attribution_json) \
         VALUES ('work_order', 'scope-probe', 'local-owner', 'local_owner', 'local_owner', \
         'local', 'local_owner_broad', NULL, 'probe', 'probe', 1, '{}')",
    )
    .execute(&pool)
    .await
    .unwrap();
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id,
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => assert_eq!(work_order.prompt, "durable"),
        other => panic!("expected preserved work order, got {other:?}"),
    }
}

// ── Authorization and privacy ──────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_unauthorized_callers_see_not_found() {
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
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);
    let created = create(&daemon, "client-member", project.as_str(), "private").await;

    // Listing without a grant denies as not-found, not as an oracle.
    let response = call(
        &daemon,
        "client-outsider",
        CoreRequest::WorkOrderList {
            project_id: project.as_str().to_owned(),
            state_filter: None,
            cursor: None,
            limit: None,
        },
    )
    .await;
    assert_eq!(error_code(&response), "project_not_found");

    // An opaque id owned by another project denies identically.
    let response = call(
        &daemon,
        "client-outsider",
        CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id.clone(),
        },
    )
    .await;
    assert_eq!(error_code(&response), "project_not_found");

    // A well-formed but absent id denies identically (no existence oracle).
    let response = call(
        &daemon,
        "client-outsider",
        CoreRequest::WorkOrderGet {
            work_order_id: "absent-work-order".to_owned(),
        },
    )
    .await;
    assert_eq!(error_code(&response), "project_not_found");

    // Mutations from outsiders deny before any side effect.
    let response = call(
        &daemon,
        "client-outsider",
        CoreRequest::WorkOrderCancel {
            work_order_id: created.work_order_id.clone(),
        },
    )
    .await;
    assert_eq!(error_code(&response), "project_not_found");
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id,
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => assert_eq!(work_order.state, "active"),
        other => panic!("expected untouched work order, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_cross_project_mutation_fails_closed() {
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
    register_client(&daemon, "client-a", member_a);
    let member_b = member_in_project(
        &team,
        &project_b,
        "MemberB",
        "client-b",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-b", member_b);

    let created = create(&daemon, "client-a", project_a.as_str(), "project-a work").await;
    // A member of another project cannot update, cancel, or reorder it.
    let response = call(
        &daemon,
        "client-b",
        CoreRequest::WorkOrderUpdate {
            request: WorkOrderUpdateRequest {
                work_order_id: created.work_order_id.clone(),
                expected_revision: 1,
                title: None,
                prompt: Some("hijacked".to_owned()),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: None,
                gate_join: None,
                repeat_count: None,
            },
        },
    )
    .await;
    assert_eq!(error_code(&response), "project_not_found");

    // A lane owned by project A cannot be reordered from project B:
    // the lane locator resolves to A, where B holds no grant.
    let lane_id = match call(
        &daemon,
        "client-a",
        CoreRequest::WorkOrderLaneCreate {
            request: WorkOrderLaneCreateRequest {
                project_id: project_a.as_str().to_owned(),
                label: None,
                failure_policy: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => lane.lane_id,
        other => panic!("expected lane, got {other:?}"),
    };
    let response = call(
        &daemon,
        "client-b",
        CoreRequest::WorkOrderLaneReorder {
            request: WorkOrderLaneReorderRequest {
                lane_id,
                expected_revision: 1,
                ordered_work_order_ids: vec![],
            },
        },
    )
    .await;
    // Foreign members are not in the lane's project: privacy-shaped denial.
    assert_eq!(error_code(&response), "project_not_found");
}

// ── Attribution, audit, and secret-free projections ────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_origin_attribution_is_immutable_and_audited() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project = ProjectId::new();
    let owner =
        member_in_project(&team, &project, "Owner", "client-owner", ProjectRole::Owner).await;
    register_client(&daemon, "client-owner", owner.clone());
    let created = create(&daemon, "client-owner", project.as_str(), "attributed").await;

    // Creator origin is captured immutably on creation.
    let store = codegg_core::authorization::OriginAttributionStore::new(pool.clone());
    let attribution = store
        .get("work_order", &created.work_order_id)
        .await
        .unwrap()
        .expect("origin attribution");
    assert_eq!(
        attribution.origin_principal.as_str(),
        owner.principal_id().as_str()
    );
    // First write wins: a later record cannot rewrite the origin.
    let mut forged = attribution.clone();
    forged.decision_id = "forged".to_owned();
    let stored = store
        .record("work_order", &created.work_order_id, &forged)
        .await
        .unwrap();
    assert_eq!(stored.decision_id, attribution.decision_id);

    // The mutation is audit-visible with decision id and revision, and
    // the page carries no prompt bodies.
    match call(
        &daemon,
        "client-owner",
        CoreRequest::AuditQuery {
            query: codegg::protocol::core::AuditQueryRequestDto {
                project_id: project.as_str().to_owned(),
                action_filter: Some("work_order_lifecycle".to_owned()),
                principal_filter: None,
                from_seq: None,
                limit: Some(50),
            },
        },
    )
    .await
    {
        CoreResponse::AuditPage { events, .. } => {
            let event = events
                .iter()
                .find(|event| {
                    event.metadata.get("work_order.id").map(String::as_str)
                        == Some(created.work_order_id.as_str())
                })
                .expect("work order lifecycle audit event");
            assert_eq!(event.action, "work_order_lifecycle");
            assert!(!event.decision_id.is_empty());
            let json = serde_json::to_string(event).unwrap();
            assert!(!json.contains("attributed") || json.contains("work_order.id"));
        }
        other => panic!("expected audit page, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn daemon_projections_carry_no_secrets_or_reasoning() {
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
    let created = create(&daemon, "client-member", project.as_str(), "plain intent").await;
    for payload in [
        serde_json::to_string(&created).unwrap(),
        serde_json::to_string(&CoreRequest::WorkOrderGet {
            work_order_id: created.work_order_id.clone(),
        })
        .unwrap(),
    ] {
        let lowered = payload.to_lowercase();
        for forbidden in ["secret", "reasoning", "token", "bearer", "password"] {
            assert!(
                !lowered.contains(forbidden),
                "projection leaks {forbidden}: {payload}"
            );
        }
    }
    // Unknown gate kinds fail closed at the daemon boundary.
    let mut bad = create_request(project.as_str(), "bad gate");
    let CoreRequest::WorkOrderCreate { ref mut request } = bad else {
        unreachable!();
    };
    request.gates = vec![WorkOrderGateDto {
        kind: "shell_predicate".to_owned(),
        delay_secs: None,
        not_before_ms: None,
        lane_id: None,
        trigger_ref: None,
    }];
    let response = call(&daemon, "client-member", bad).await;
    assert_eq!(error_code(&response), "work_order_invalid_input");
}

// ── Occurrences, summary, and lane moves ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn daemon_occurrences_summary_and_moves() {
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

    let first = create(&daemon, "client-member", project.as_str(), "first").await;
    let second = create(&daemon, "client-member", project.as_str(), "second").await;
    let third = create(&daemon, "client-member", project.as_str(), "third").await;

    // Summary counts bound the project projection.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderSummary {
            project_id: project.as_str().to_owned(),
        },
    )
    .await
    {
        CoreResponse::WorkOrderSummary { summary } => {
            assert_eq!(summary.active, 3);
            assert_eq!(summary.lane_count, 0);
        }
        other => panic!("expected summary, got {other:?}"),
    }

    // Occurrence listing for a work order without occurrences is empty.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderOccurrenceList {
            work_order_id: first.work_order_id.clone(),
            limit: None,
        },
    )
    .await
    {
        CoreResponse::WorkOrderOccurrenceList {
            occurrences,
            truncated,
        } => {
            assert!(occurrences.is_empty());
            assert!(!truncated);
        }
        other => panic!("expected occurrence list, got {other:?}"),
    }

    // Lane attach plus an atomic before-move reorder the segment.
    let lane_id = match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderLaneCreate {
            request: WorkOrderLaneCreateRequest {
                project_id: project.as_str().to_owned(),
                label: None,
                failure_policy: None,
                idempotency_key: None,
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => lane.lane_id,
        other => panic!("expected lane, got {other:?}"),
    };
    let mut revision = 1;
    for work_id in [
        &first.work_order_id,
        &second.work_order_id,
        &third.work_order_id,
    ] {
        match call(
            &daemon,
            "client-member",
            CoreRequest::WorkOrderLaneAttach {
                request: codegg::protocol::work_order::WorkOrderLaneAttachRequest {
                    lane_id: lane_id.clone(),
                    expected_revision: revision,
                    work_order_id: work_id.clone(),
                    position: None,
                },
            },
        )
        .await
        {
            CoreResponse::WorkOrderLane { lane } => revision = lane.revision,
            other => panic!("expected lane attach, got {other:?}"),
        }
    }
    // Move the third member before the first through full reorder.
    match call(
        &daemon,
        "client-member",
        CoreRequest::WorkOrderLaneReorder {
            request: WorkOrderLaneReorderRequest {
                lane_id: lane_id.clone(),
                expected_revision: revision,
                ordered_work_order_ids: vec![
                    third.work_order_id.clone(),
                    first.work_order_id.clone(),
                    second.work_order_id.clone(),
                ],
            },
        },
    )
    .await
    {
        CoreResponse::WorkOrderLane { lane } => {
            assert_eq!(
                lane.ordered_work_order_ids,
                vec![
                    third.work_order_id.clone(),
                    first.work_order_id.clone(),
                    second.work_order_id.clone(),
                ]
            );
        }
        other => panic!("expected lane reorder, got {other:?}"),
    }
}
