//! Project Work Orders M004 — global Workspace dashboard projection.
//!
//! Daemon-boundary proof for the plan: one bounded enumeration-style
//! aggregate over authorized projects with coarse counts and a closed
//! status vocabulary, privacy-filtered to `project.read`, per-row
//! counts gated on `session.read`, deterministic paged ordering, and
//! no project activation (no LSP/Git/provider/build/workspace
//! probing). The TUI consumes exactly this surface instead of N+1
//! per-project fan-out.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::work_order::{
    ProjectActivitySummaryDto, WorkOrderCreateRequest, WorkOrderGateDto,
    WORKSPACE_DASHBOARD_STATUS_CODES,
};
use codegg_core::identity::ProjectId;
use codegg_core::team::{ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};
use codegg_core::workspace::{SqliteWorkspaceStore, WorkspaceId, WorkspaceRecord, WorkspaceStore};

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn test_workspace_at(pool: &sqlx::SqlitePool, root: &str) -> WorkspaceId {
    let now = chrono::Utc::now();
    let workspace = WorkspaceRecord {
        id: WorkspaceId::new(),
        canonical_root: std::path::PathBuf::from(root),
        display_name: "Dashboard test workspace".to_string(),
        created_at: now,
        last_opened_at: now,
        archived_at: None,
    };
    let store = SqliteWorkspaceStore::new(pool.clone());
    WorkspaceStore::upsert(&store, &workspace)
        .await
        .expect("test workspace registration");
    workspace.id
}

async fn catalog_project(pool: &sqlx::SqlitePool, name: &str) -> ProjectId {
    // One workspace per project: registration deduplicates on the
    // workspace binding (one workspace backs one project), so sharing
    // a workspace would converge on a single catalog row.
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let workspace = test_workspace_at(pool, &format!("/tmp/dashboard-{n}")).await;
    let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
    let record = catalog
        .register_local_project(
            codegg_core::project_catalog::RegisterLocalProject {
                display_name: name.to_string(),
                description: None,
                tags: Vec::new(),
                primary_repository_id: None,
            },
            &workspace,
            "dashboard-test",
        )
        .await
        .expect("test project registration");
    record.project_id
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

/// Create one waiting work order behind a long delay gate so the M002
/// coordinator never materializes it during the test. Returns the
/// durable template DTO.
async fn create_delayed(
    daemon: &CoreDaemon,
    client: &str,
    project: &str,
) -> codegg::protocol::work_order::WorkOrderDto {
    match call(
        daemon,
        client,
        CoreRequest::WorkOrderCreate {
            request: WorkOrderCreateRequest {
                project_id: project.to_owned(),
                title: Some("Future dashboard work".to_owned()),
                prompt: "dashboard probe prompt".to_owned(),
                requested_model: None,
                requested_approval: None,
                requested_sandbox: None,
                workspace_policy: None,
                gates: vec![WorkOrderGateDto {
                    kind: "delay".to_owned(),
                    delay_secs: Some(86_400),
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
        },
    )
    .await
    {
        CoreResponse::WorkOrder { work_order, .. } => work_order,
        other => panic!("expected work order, got {other:?}"),
    }
}

async fn dashboard(
    daemon: &CoreDaemon,
    client: &str,
    limit: Option<u32>,
) -> (Vec<ProjectActivitySummaryDto>, bool, Option<String>) {
    match call(
        daemon,
        client,
        CoreRequest::WorkspaceDashboard {
            cursor: None,
            limit,
            include_archived: false,
        },
    )
    .await
    {
        CoreResponse::WorkspaceDashboard {
            rows,
            truncated,
            next_cursor,
        } => (rows, truncated, next_cursor),
        other => panic!("expected workspace dashboard, got {other:?}"),
    }
}

fn row_by_id<'a>(rows: &'a [ProjectActivitySummaryDto], id: &str) -> &'a ProjectActivitySummaryDto {
    rows.iter()
        .find(|row| row.project_id == id)
        .expect("project row present")
}

#[tokio::test(flavor = "current_thread")]
async fn owner_sees_bounded_redacted_rows_with_counts() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project_a = catalog_project(&pool, "Alpha").await;
    let project_b = catalog_project(&pool, "Beta").await;
    create_delayed(&daemon, "local-daemon", project_a.as_str()).await;

    let (rows, truncated, next_cursor) = dashboard(&daemon, "local-daemon", None).await;
    assert!(!truncated);
    assert!(next_cursor.is_none());
    assert_eq!(rows.len(), 2);

    let row_a = row_by_id(&rows, project_a.as_str());
    assert_eq!(row_a.display_name, "Alpha");
    assert_eq!(row_a.lifecycle, "active");
    assert!(row_a.counts_visible);
    // M001 lifecycle truth: a delayed template is durable future work
    // (`future_work_order_count`); `waiting_work_order_count` counts
    // release-pending occurrence instances (none until the M002
    // coordinator records them via `create_occurrence`).
    assert_eq!(row_a.waiting_work_order_count, 0);
    assert_eq!(row_a.future_work_order_count, 1);
    assert_eq!(row_a.coarse_status_code, "waiting");
    assert!(row_a.last_activity_at.is_some());
    assert!(row_a.is_redacted());
    assert!(WORKSPACE_DASHBOARD_STATUS_CODES.contains(&row_a.coarse_status_code.as_str()));

    let row_b = row_by_id(&rows, project_b.as_str());
    assert_eq!(row_b.coarse_status_code, "idle");

    // Wire redaction: coarse counts and closed labels only — no
    // prompt, secret, path, diff, token, or reasoning content.
    let json = serde_json::to_string(&rows).expect("rows serialize");
    for forbidden in [
        "dashboard probe prompt",
        "secret",
        "reasoning",
        "token",
        "diff",
        "canonical_root",
        "/tmp",
    ] {
        assert!(
            !json.contains(forbidden),
            "dashboard row leaks {forbidden:?}"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn member_sees_only_their_project() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project_a = catalog_project(&pool, "Alpha").await;
    let project_b = catalog_project(&pool, "Beta").await;
    let member = member_in_project(
        &team,
        &project_a,
        "Member",
        "client-member",
        ProjectRole::Contributor,
    )
    .await;
    register_client(&daemon, "client-member", member);

    let (rows, _, _) = dashboard(&daemon, "client-member", None).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].project_id, project_a.as_str());
    // Unauthorized projects are absent (privacy-equivalent to absent):
    // the member cannot even name project B through this surface.
    assert!(rows.iter().all(|row| row.project_id != project_b.as_str()));
}

#[tokio::test(flavor = "current_thread")]
async fn viewer_sees_counts_explicit_decision() {
    // Explicit §6 decision: the role matrix grants Viewers
    // `session.read`, so Viewers see task/session *counts* (never
    // content). Presence-only zeroing (`counts_visible == false`) is
    // reserved for future least-privilege grants and is unit-pinned in
    // `daemon_workspace_dashboard.rs`.
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let team = TeamStore::new(pool.clone());
    let project_a = catalog_project(&pool, "Alpha").await;
    let viewer = member_in_project(
        &team,
        &project_a,
        "Viewer",
        "client-viewer",
        ProjectRole::Viewer,
    )
    .await;
    register_client(&daemon, "client-viewer", viewer);
    create_delayed(&daemon, "local-daemon", project_a.as_str()).await;

    let (rows, _, _) = dashboard(&daemon, "client-viewer", None).await;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].counts_visible);
    assert_eq!(rows[0].future_work_order_count, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn pagination_is_deterministic_and_bounded() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    for name in ["P1", "P2", "P3", "P4", "P5"] {
        catalog_project(&pool, name).await;
    }
    // Full read twice: identical deterministic order.
    let (first, _, _) = dashboard(&daemon, "local-daemon", None).await;
    let (second, _, _) = dashboard(&daemon, "local-daemon", None).await;
    assert_eq!(first.len(), 5);
    let order: Vec<&str> = first.iter().map(|row| row.project_id.as_str()).collect();
    let order2: Vec<&str> = second.iter().map(|row| row.project_id.as_str()).collect();
    assert_eq!(order, order2);

    // Page through with limit 2: union covers the full set exactly.
    let mut seen = Vec::new();
    let mut cursor = None;
    for _ in 0..4 {
        let response = call(
            &daemon,
            "local-daemon",
            CoreRequest::WorkspaceDashboard {
                cursor: cursor.clone(),
                limit: Some(2),
                include_archived: false,
            },
        )
        .await;
        match response {
            CoreResponse::WorkspaceDashboard {
                rows,
                truncated,
                next_cursor,
            } => {
                assert!(rows.len() <= 2);
                seen.extend(rows.iter().map(|row| row.project_id.clone()));
                if truncated {
                    cursor = next_cursor;
                    assert!(cursor.is_some());
                } else {
                    assert!(next_cursor.is_none());
                    break;
                }
            }
            other => panic!("expected workspace dashboard, got {other:?}"),
        }
    }
    seen.sort();
    let mut expected: Vec<String> = order.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(seen, expected);

    // Unknown cursor restarts from the beginning (fail-closed paging).
    match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkspaceDashboard {
            cursor: Some("missing-project".to_string()),
            limit: Some(2),
            include_archived: false,
        },
    )
    .await
    {
        CoreResponse::WorkspaceDashboard { rows, .. } => assert_eq!(rows.len(), 2),
        other => panic!("expected workspace dashboard, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn attention_failed_and_permission_badges() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project_a = catalog_project(&pool, "Alpha").await;
    let template_a = create_delayed(&daemon, "local-daemon", project_a.as_str()).await;
    let template_b = create_delayed(&daemon, "local-daemon", project_a.as_str()).await;

    // Record release-pending occurrence instances through the M001
    // primitive (the M002 coordinator owns claim transitions; the
    // fixture then drives the rows into coordinator-written states).
    let work_order_a =
        codegg_core::identity::WorkOrderId::parse(&template_a.work_order_id).unwrap();
    let work_order_b =
        codegg_core::identity::WorkOrderId::parse(&template_b.work_order_id).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    daemon
        .work_orders
        .create_occurrence(&project_a, &work_order_a, None, now)
        .await
        .expect("occurrence a");
    daemon
        .work_orders
        .create_occurrence(&project_a, &work_order_b, None, now)
        .await
        .expect("occurrence b");

    // Drive one occurrence to needs_attention and one to failed
    // through the durable store (same states the coordinator writes).
    sqlx::query(
        "UPDATE work_order_occurrence SET state = 'needs_attention', attention_code = 'model_unavailable', updated_at = 100 WHERE project_id = ? AND id = (SELECT id FROM work_order_occurrence WHERE project_id = ? ORDER BY id LIMIT 1)",
    )
    .bind(project_a.as_str())
    .bind(project_a.as_str())
    .execute(&pool)
    .await
    .expect("attention update");
    sqlx::query(
        "UPDATE work_order_occurrence SET state = 'failed', updated_at = 101 WHERE project_id = ? AND id = (SELECT id FROM work_order_occurrence WHERE project_id = ? AND state = 'waiting' ORDER BY id LIMIT 1)",
    )
    .bind(project_a.as_str())
    .bind(project_a.as_str())
    .execute(&pool)
    .await
    .expect("failed update");

    // One live session with a pending permission (count only, no
    // content) via the in-memory registry — no activation involved.
    let runtime = daemon.sessions.get_or_create(
        "sess-attention",
        codegg_core::workspace::WorkspaceId::new(),
        std::path::PathBuf::from("/tmp"),
        project_a.as_str().to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    runtime.pending_permissions.insert("perm-1".to_string());

    let (rows, _, _) = dashboard(&daemon, "local-daemon", None).await;
    let row = row_by_id(&rows, project_a.as_str());
    assert_eq!(row.needs_attention_count, 2);
    assert_eq!(row.pending_permission_count, 1);
    // Precedence: permission dominates attention/failed.
    assert_eq!(row.coarse_status_code, "permission");

    runtime.pending_permissions.remove("perm-1");
    let (rows, _, _) = dashboard(&daemon, "local-daemon", None).await;
    assert_eq!(
        row_by_id(&rows, project_a.as_str()).coarse_status_code,
        "attention"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn running_session_counts_come_from_registry_only() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project_a = catalog_project(&pool, "Alpha").await;

    let runtime = daemon.sessions.get_or_create(
        "sess-running",
        codegg_core::workspace::WorkspaceId::new(),
        std::path::PathBuf::from("/tmp"),
        project_a.as_str().to_string(),
        std::path::PathBuf::from("/tmp"),
    );
    *runtime.status.write().await = codegg::core::session_runtime::RuntimeSessionStatus::Running;

    let (rows, _, _) = dashboard(&daemon, "local-daemon", None).await;
    let row = row_by_id(&rows, project_a.as_str());
    assert_eq!(row.running_session_count, 1);
    assert_eq!(row.coarse_status_code, "running");
}

#[tokio::test(flavor = "current_thread")]
async fn archived_projects_filter_and_report() {
    let pool = test_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    let project_a = catalog_project(&pool, "Alpha").await;
    let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
    catalog
        .archive_project(&project_a, "dashboard-test")
        .await
        .expect("archive");

    let (rows, _, _) = dashboard(&daemon, "local-daemon", None).await;
    assert!(rows.is_empty());

    match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkspaceDashboard {
            cursor: None,
            limit: None,
            include_archived: true,
        },
    )
    .await
    {
        CoreResponse::WorkspaceDashboard { rows, .. } => {
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].lifecycle, "archived");
            assert_eq!(rows[0].coarse_status_code, "archived");
        }
        other => panic!("expected workspace dashboard, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn pool_less_daemon_reports_unavailable() {
    let daemon = CoreDaemon::new(None, None, None);
    match call(
        &daemon,
        "local-daemon",
        CoreRequest::WorkspaceDashboard {
            cursor: None,
            limit: None,
            include_archived: false,
        },
    )
    .await
    {
        CoreResponse::Error { code, .. } => assert_eq!(code, "workspace_dashboard_unavailable"),
        other => panic!("expected unavailable error, got {other:?}"),
    }
}
