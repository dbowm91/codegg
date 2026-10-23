//! Project Work Orders M006 — agent WorkOrder tool and atomic batches.
//!
//! Dedicated `work_order` tool (distinct from delegated `task`), bounded
//! single/batch creation through the canonical M001 service, atomic lane
//! ordering with `start_first_now`, parent lineage, authority narrowing,
//! fan-out/depth/repeat/total-bytes limits, invocation idempotency, and
//! compact results. No direct execution: first-release still requires the
//! M002 coordinator.

use codegg::tool::work_order::{
    WorkOrderTool, MAX_AGENT_BATCH_TOTAL_BYTES, MAX_AGENT_CREATED_PER_TURN,
    MAX_AGENT_DESCENDANTS_PER_ROOT, MAX_AGENT_PENDING_PER_PROJECT, MAX_AGENT_REPEAT_COUNT,
    MAX_AGENT_WORK_ORDER_BATCH, MAX_AGENT_WORK_ORDER_DEPTH,
};
use codegg::tool::Tool;
use codegg::tool::{ToolBackendKind, ToolExecutionContext};
use codegg_core::identity::{PrincipalId, ProjectId};
use codegg_core::work_order::{GateKind, OccurrenceState, WorkOrderService};
use serde_json::json;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

fn project() -> ProjectId {
    ProjectId::parse("project-m006").unwrap()
}

fn other_project() -> ProjectId {
    ProjectId::parse("project-other").unwrap()
}

fn creator() -> PrincipalId {
    PrincipalId::parse("local-owner").unwrap()
}

fn tool_for(pool: &sqlx::SqlitePool, turn: &str) -> WorkOrderTool {
    WorkOrderTool::new(Some(pool.clone()))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some(turn.to_string()))
        .with_effective_model(Some("openai/gpt-4o".to_string()))
        .with_creator(Some(creator()))
}

fn ctx_for(invocation: &str, turn: &str) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
    ctx.session_id = Some("session-1".to_string());
    ctx.turn_id = Some(turn.to_string());
    ctx.invocation_key = Some(invocation.to_string());
    ctx.permission_mode = Some("interactive".to_string());
    ctx.sandbox_profile = Some("workspace_write".to_string());
    ctx.caller_class = Some("agent".to_string());
    ctx
}

fn ctx_yolo(invocation: &str, turn: &str) -> ToolExecutionContext {
    let mut ctx = ctx_for(invocation, turn);
    ctx.permission_mode = Some("yolo".to_string());
    ctx.sandbox_profile = Some("full_host".to_string());
    ctx
}

// ── Tool contract ────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn factory_exposes_work_order_without_changing_task() {
    let pool = test_pool().await;
    let config = codegg::config::schema::Config::default();
    let execution = {
        let root = std::env::temp_dir().join(format!("m006-{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).unwrap();
        let id = codegg_core::workspace::WorkspaceId::new_unchecked("ws-m006".to_string());
        let record = std::sync::Arc::new(codegg_core::workspace::WorkspaceRecord {
            id,
            canonical_root: root.clone(),
            display_name: "m006".to_string(),
            created_at: chrono::Utc::now(),
            last_opened_at: chrono::Utc::now(),
            archived_at: None,
        });
        codegg_core::workspace::ExecutionContext::new(
            record,
            Some("session-1".to_string()),
            Default::default(),
        )
    };
    let (registry, _) = codegg::tool::factory::build_session_tool_registry(
        &config,
        Some(pool),
        "session-1",
        None,
        codegg_core::model_profile::types::TaskStatePolicy::explicit_todo(),
        Some("openai/gpt-4o".to_string()),
        execution,
        codegg::tool::factory::SessionToolContext {
            project_id: Some(project()),
            turn_id: Some("turn-1".to_string()),
            ..Default::default()
        },
    );
    assert!(
        registry.contains("work_order"),
        "work_order tool registered"
    );
    assert!(registry.contains("task"), "task tool still registered");
    let work_order = registry.get("work_order").expect("work_order present");
    let task = registry.get("task").expect("task present");
    assert_eq!(work_order.name(), "work_order");
    assert_eq!(task.name(), "task");
    assert_ne!(work_order.name(), task.name());
    assert!(
        work_order.description().contains("NOT delegated")
            || work_order.description().contains("not delegated")
            || work_order.description().contains("NOT"),
        "description distinguishes from TaskTool: {}",
        work_order.description()
    );
    // Schemas are bounded and descriptive.
    let params = work_order.parameters();
    let actions = params["properties"]["action"]["enum"]
        .as_array()
        .expect("action enum")
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>();
    for expected in ["create", "create_batch", "list", "get"] {
        assert!(actions.contains(&expected), "schema has {expected}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_action_is_rejected() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-1");
    let err = tool
        .execute_structured(
            json!({"action": "teleport"}),
            Some(ctx_for("inv-1", "turn-1")),
        )
        .await
        .expect_err("unknown action rejected");
    assert!(err.to_string().contains("unsupported"));
}

#[tokio::test(flavor = "current_thread")]
async fn host_owned_fields_are_rejected() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-1");
    for field in [
        "parent_session_id",
        "parent_turn_id",
        "parent_work_order_id",
        "gates",
        "trigger_ref",
    ] {
        let mut input = json!({"action": "create", "prompt": "do work"});
        input[field] = json!("smuggled");
        let err = tool
            .execute_structured(input, Some(ctx_for("inv-1", "turn-1")))
            .await
            .expect_err("host-owned field rejected");
        assert!(
            err.to_string().contains("host-owned"),
            "field {field}: {err}"
        );
    }
}

// ── Single creation ──────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn single_create_records_lineage_and_compact_result() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-1");
    let output = tool
        .execute_structured(
            json!({"action": "create", "prompt": "implement feature X", "title": "Feature X"}),
            Some(ctx_for("inv-single-1", "turn-1")),
        )
        .await
        .expect("create")
        .output;
    let value: serde_json::Value = serde_json::from_str(&output).expect("json result");
    assert_eq!(value["created"], 1);
    assert!(!output.contains("implement feature X"), "prompt not echoed");
    let id = value["items"][0]["id"].as_str().expect("id");
    assert_eq!(value["items"][0]["short_title"], "Feature X");
    // Lineage round-trips through the durable service.
    let service = WorkOrderService::with_defaults(Some(pool));
    let stored = service
        .get_work_order(
            &project(),
            &codegg_core::identity::WorkOrderId::parse(id).unwrap(),
        )
        .await
        .unwrap()
        .expect("stored");
    assert_eq!(stored.parent_session_id.as_deref(), Some("session-1"));
    assert_eq!(stored.parent_turn_id.as_deref(), Some("turn-1"));
    assert_eq!(stored.creator_principal, creator());
    assert_eq!(stored.requested_model.as_deref(), Some("openai/gpt-4o"));
    // One waiting occurrence exists for the coordinator.
    let page = service
        .list_occurrences(&project(), &stored.id, Some(4))
        .await
        .unwrap();
    assert_eq!(page.occurrences.len(), 1);
    assert_eq!(page.occurrences[0].state, OccurrenceState::Waiting);
}

#[tokio::test(flavor = "current_thread")]
async fn default_model_comes_from_session_not_task_preference() {
    let pool = test_pool().await;
    // Effective session model is claude/sonnet; the human Task-mode
    // preference (if any) must never leak into agent-created rows.
    let tool = WorkOrderTool::new(Some(pool.clone()))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some("turn-1".to_string()))
        .with_effective_model(Some("claude/sonnet".to_string()))
        .with_creator(Some(creator()));
    tool.execute_structured(
        json!({"action": "create", "prompt": "session model default"}),
        Some(ctx_for("inv-model-1", "turn-1")),
    )
    .await
    .expect("create");
    let service = WorkOrderService::with_defaults(Some(pool));
    let page = service
        .list_work_orders(&project(), None, None, Some(10))
        .await
        .unwrap();
    assert_eq!(page.work_orders.len(), 1);
    assert_eq!(
        page.work_orders[0].requested_model.as_deref(),
        Some("claude/sonnet")
    );
}

// ── Batch ────────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn batch_creates_ordered_lane_with_first_immediate() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-batch");
    let output = tool
        .execute_structured(
            json!({
                "action": "create_batch",
                "items": [
                    {"prompt": "plan one", "title": "One"},
                    {"prompt": "plan two", "title": "Two"},
                    {"prompt": "plan three", "title": "Three"},
                ],
                "sequence": {"new_lane_label": "plans", "start_first_now": true},
            }),
            Some(ctx_for("inv-batch-1", "turn-batch")),
        )
        .await
        .expect("batch")
        .output;
    let value: serde_json::Value = serde_json::from_str(&output).expect("json");
    assert_eq!(value["created"], 3);
    assert_eq!(value["first_release"], "immediate");
    assert!(value["lane"].is_string());
    assert!(!output.contains("plan one"), "prompts not echoed");
    let ids: Vec<String> = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 3);
    // Lane order matches submission order.
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let lane_id =
        codegg_core::identity::SequenceLaneId::parse(value["lane"].as_str().unwrap()).unwrap();
    let lane = service
        .get_lane(&project(), &lane_id)
        .await
        .unwrap()
        .expect("lane");
    let ordered: Vec<String> = lane
        .ordered_work_order_ids
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(ordered, ids);
    // Gate shape: first immediate, rest sequence_ready on the lane.
    let first = service
        .get_work_order(
            &project(),
            &codegg_core::identity::WorkOrderId::parse(&ids[0]).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(first
        .gates
        .gates
        .iter()
        .any(|g| g.kind == GateKind::Immediate));
    for id in ids.iter().skip(1) {
        let wo = service
            .get_work_order(
                &project(),
                &codegg_core::identity::WorkOrderId::parse(id).unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
        assert!(
            wo.gates
                .gates
                .iter()
                .any(|g| g.kind == GateKind::SequenceReady),
            "member {id} is sequence-gated"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn batch_start_first_now_releases_only_first_through_coordinator() {
    use codegg::core::work_order_coordinator::WorkOrderCoordinator;
    use std::sync::Arc;

    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-coord");
    let output = tool
        .execute_structured(
            json!({
                "action": "create_batch",
                "items": [
                    {"prompt": "first plan", "title": "First"},
                    {"prompt": "second plan", "title": "Second"},
                ],
                "sequence": {"new_lane_label": "queue", "start_first_now": true},
            }),
            Some(ctx_for("inv-coord-1", "turn-coord")),
        )
        .await
        .expect("batch")
        .output;
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    let ids: Vec<String> = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap().to_owned())
        .collect();
    // Coordinator evaluation (no direct execution from the tool):
    // first immediate is ready, second is held on the lane.
    let service = Arc::new(WorkOrderService::with_defaults(Some(pool)));
    let coordinator = WorkOrderCoordinator::new(service.clone());
    let ready = coordinator
        .evaluate_due_for_project(&project(), 9_999_999)
        .await
        .expect("evaluate");
    let ready_ids: Vec<String> = ready
        .iter()
        .map(|(wo, _)| wo.id.as_str().to_owned())
        .collect();
    assert!(ready_ids.contains(&ids[0]), "first is ready: {ready_ids:?}");
    assert!(!ready_ids.contains(&ids[1]), "second held: {ready_ids:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_item_aborts_entire_batch() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-abort");
    let err = tool
        .execute_structured(
            json!({
                "action": "create_batch",
                "items": [
                    {"prompt": "good one", "title": "Good"},
                    {"prompt": "", "title": "Empty prompt fails"},
                    {"prompt": "good three", "title": "Good3"},
                ],
                "sequence": {"new_lane_label": "abort-lane", "start_first_now": true},
            }),
            Some(ctx_for("inv-abort-1", "turn-abort")),
        )
        .await
        .expect_err("invalid batch rejected");
    assert!(!err.to_string().is_empty());
    let service = WorkOrderService::with_defaults(Some(pool));
    let page = service
        .list_work_orders(&project(), None, None, Some(32))
        .await
        .unwrap();
    assert_eq!(page.work_orders.len(), 0, "no partial commit");
}

#[tokio::test(flavor = "current_thread")]
async fn lane_conflict_aborts_entire_batch() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-conflict");
    let err = tool
        .execute_structured(
            json!({
                "action": "create_batch",
                "items": [{"prompt": "one"}],
                "sequence": {"existing_lane_id": "lane_does_not_exist", "start_first_now": true},
            }),
            Some(ctx_for("inv-conflict-1", "turn-conflict")),
        )
        .await
        .expect_err("unknown lane rejected");
    assert!(err.to_string().contains("not found"));
    let service = WorkOrderService::with_defaults(Some(pool));
    let page = service
        .list_work_orders(&project(), None, None, Some(32))
        .await
        .unwrap();
    assert_eq!(page.work_orders.len(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn batch_bounds_are_exact() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-bounds");
    // Item count cap.
    let too_many: Vec<serde_json::Value> = (0..MAX_AGENT_WORK_ORDER_BATCH + 1)
        .map(|i| json!({"prompt": format!("prompt {i}")}))
        .collect();
    let err = tool
        .execute_structured(
            json!({"action": "create_batch", "items": too_many}),
            Some(ctx_for("inv-bounds-count", "turn-bounds")),
        )
        .await
        .expect_err("count cap");
    assert!(err.to_string().contains("1..="));
    // Total bytes cap.
    let big = "x".repeat(20 * 1024);
    let items: Vec<serde_json::Value> = (0..4).map(|_| json!({"prompt": big})).collect();
    #[allow(clippy::assertions_on_constants)]
    {
        assert!(4 * 20 * 1024 > MAX_AGENT_BATCH_TOTAL_BYTES);
    }
    let err = tool
        .execute_structured(
            json!({"action": "create_batch", "items": items}),
            Some(ctx_for("inv-bounds-bytes", "turn-bounds")),
        )
        .await
        .expect_err("bytes cap");
    assert!(err.to_string().contains("total bound"));
    // Repeat cap.
    let err = tool
        .execute_structured(
            json!({"action": "create", "prompt": "repeat", "repeat_count": MAX_AGENT_REPEAT_COUNT + 1}),
            Some(ctx_for("inv-bounds-repeat", "turn-bounds")),
        )
        .await
        .expect_err("repeat cap");
    assert!(err.to_string().contains("repeat_count"));
}

// ── Idempotency ──────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn same_invocation_retry_returns_same_ids() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-idem");
    let input = json!({
        "action": "create_batch",
        "items": [{"prompt": "retry me", "title": "Retry"}],
        "sequence": {"new_lane_label": "retry-lane", "start_first_now": true},
    });
    let first = tool
        .execute_structured(input.clone(), Some(ctx_for("inv-retry-1", "turn-idem")))
        .await
        .expect("first")
        .output;
    let second = tool
        .execute_structured(input, Some(ctx_for("inv-retry-1", "turn-idem")))
        .await
        .expect("retry")
        .output;
    let first_json: serde_json::Value = serde_json::from_str(&first).unwrap();
    let second_json: serde_json::Value = serde_json::from_str(&second).unwrap();
    assert_eq!(first_json["items"], second_json["items"]);
    let service = WorkOrderService::with_defaults(Some(pool));
    let page = service
        .list_work_orders(&project(), None, None, Some(32))
        .await
        .unwrap();
    assert_eq!(page.work_orders.len(), 1, "no duplicate");
}

#[tokio::test(flavor = "current_thread")]
async fn changed_payload_under_same_key_conflicts() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-conflict-key");
    tool.execute_structured(
        json!({"action": "create", "prompt": "original", "idempotency_key": "key-1"}),
        Some(ctx_for("inv-conflict-key", "turn-conflict-key")),
    )
    .await
    .expect("first");
    let err = tool
        .execute_structured(
            json!({"action": "create", "prompt": "different payload", "idempotency_key": "key-1"}),
            Some(ctx_for("inv-conflict-key", "turn-conflict-key")),
        )
        .await
        .expect_err("conflict");
    assert!(err.to_string().contains("idempotency conflict"));
}

#[tokio::test(flavor = "current_thread")]
async fn identical_text_in_distinct_calls_creates_distinct_rows() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-distinct");
    for invocation in ["inv-distinct-1", "inv-distinct-2"] {
        tool.execute_structured(
            json!({"action": "create", "prompt": "same text"}),
            Some(ctx_for(invocation, "turn-distinct")),
        )
        .await
        .expect("create");
    }
    let service = WorkOrderService::with_defaults(Some(pool));
    let page = service
        .list_work_orders(&project(), None, None, Some(32))
        .await
        .unwrap();
    assert_eq!(page.work_orders.len(), 2);
}

// ── Authority ────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cross_project_target_is_rejected() {
    let pool = test_pool().await;
    // Bound tool is scoped to project-m006; a forged project_id must fail
    // even though the field itself is host-owned (defense in depth).
    let tool = WorkOrderTool::new(Some(pool))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some("turn-1".to_string()));
    let err = tool
        .execute_structured(
            json!({"action": "list", "project_scope": "other"}),
            Some(ctx_for("inv-xproj", "turn-1")),
        )
        .await
        .expect_err("cross-project rejected");
    assert!(err.to_string().contains("cross-project"));
    let _ = other_project();
}

#[tokio::test(flavor = "current_thread")]
async fn broader_yolo_and_full_host_are_rejected() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-ceiling");
    let ctx = ctx_for("inv-ceiling-1", "turn-ceiling");
    for input in [
        json!({"action": "create", "prompt": "x", "requested_approval": "yolo"}),
        json!({"action": "create", "prompt": "x", "requested_sandbox": "full_host"}),
    ] {
        let err = tool
            .execute_structured(input, Some(ctx.clone()))
            .await
            .expect_err("ceiling enforced");
        assert!(
            err.to_string().contains("ceiling"),
            "expected ceiling error, got {err}"
        );
    }
    // Yolo caller may request yolo.
    let yolo_tool = tool_for(&pool, "turn-ceiling");
    yolo_tool
        .execute_structured(
            json!({"action": "create", "prompt": "yolo ok", "requested_approval": "yolo", "requested_sandbox": "full_host"}),
            Some(ctx_yolo("inv-ceiling-yolo", "turn-ceiling")),
        )
        .await
        .expect("yolo caller may request yolo");
}

#[tokio::test(flavor = "current_thread")]
async fn unauthorized_model_is_rejected_when_allowlist_set() {
    let pool = test_pool().await;
    let tool = WorkOrderTool::new(Some(pool))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some("turn-allow".to_string()))
        .with_effective_model(Some("openai/gpt-4o".to_string()))
        .with_allowed_models(Some(vec!["openai/gpt-4o".to_string()]));
    let err = tool
        .execute_structured(
            json!({"action": "create", "prompt": "x", "requested_model": "evil/other"}),
            Some(ctx_for("inv-allow-1", "turn-allow")),
        )
        .await
        .expect_err("allowlist enforced");
    assert!(err.to_string().contains("not available"));
}

#[tokio::test(flavor = "current_thread")]
async fn trigger_material_never_passes_through_tool() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-trigger");
    // No trigger-bearing output exists on create/list/get.
    let created = tool
        .execute_structured(
            json!({"action": "create", "prompt": "no triggers"}),
            Some(ctx_for("inv-trig-1", "turn-trigger")),
        )
        .await
        .expect("create")
        .output;
    assert!(!created.contains("cggtr_"));
    assert!(!created.contains("secret"));
    assert!(!created.contains("verifier"));
    let listed = tool
        .execute_structured(
            json!({"action": "list"}),
            Some(ctx_for("inv-trig-2", "turn-trigger")),
        )
        .await
        .expect("list")
        .output;
    assert!(!listed.contains("cggtr_"));
    assert!(!listed.contains("verifier"));
}

#[tokio::test(flavor = "current_thread")]
async fn turn_budget_survives_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("m006.sqlite");
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("file pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    let tool = WorkOrderTool::new(Some(pool.clone()))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some("turn-budget".to_string()))
        .with_creator(Some(creator()));
    // Fill the per-turn budget exactly.
    for i in 0..MAX_AGENT_CREATED_PER_TURN {
        tool.execute_structured(
            json!({"action": "create", "prompt": format!("budget {i}")}),
            Some(ctx_for(&format!("inv-budget-{i}"), "turn-budget")),
        )
        .await
        .unwrap_or_else(|e| panic!("budget item {i}: {e}"));
    }
    // Simulate restart: new pool + new tool over the same file.
    drop(tool);
    let pool2 = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("reopen");
    codegg_core::session::schema::migrate(&pool2)
        .await
        .expect("remigrate");
    let tool2 = WorkOrderTool::new(Some(pool2))
        .with_project(Some(project()))
        .with_session(Some("session-1".to_string()))
        .with_turn(Some("turn-budget".to_string()))
        .with_creator(Some(creator()));
    let err = tool2
        .execute_structured(
            json!({"action": "create", "prompt": "over budget"}),
            Some(ctx_for("inv-budget-over", "turn-budget")),
        )
        .await
        .expect_err("budget enforced after restart");
    assert!(err.to_string().contains("budget"));
}

#[tokio::test(flavor = "current_thread")]
async fn depth_limit_blocks_nested_self_replication() {
    let pool = test_pool().await;
    let service = WorkOrderService::with_defaults(Some(pool.clone()));
    let now = 1_000;
    // Build a durable chain of depth MAX_AGENT_WORK_ORDER_DEPTH.
    let mut parent: Option<codegg_core::identity::WorkOrderId> = None;
    for i in 0..MAX_AGENT_WORK_ORDER_DEPTH {
        let created = service
            .create_work_order(
                &project(),
                &creator(),
                codegg_core::work_order::NewWorkOrder {
                    title: None,
                    prompt: format!("depth {i}"),
                    requested_model: None,
                    requested_approval: None,
                    requested_sandbox: None,
                    workspace_policy: None,
                    gates: codegg_core::work_order::ReleaseGateSet::immediate(),
                    repeat_count: 1,
                    sequence_lane_id: None,
                    parent_session_id: Some("session-1".to_string()),
                    parent_turn_id: Some("turn-depth".to_string()),
                    parent_work_order_id: parent.clone(),
                    idempotency_key: Some(format!("depth-{i}")),
                },
                now + i as i64,
            )
            .await
            .unwrap();
        parent = Some(created.work_order.id);
    }
    // A tool bound inside the deepest session must refuse to nest further.
    let tool = WorkOrderTool::new(Some(pool))
        .with_project(Some(project()))
        .with_session(Some("session-deep".to_string()))
        .with_turn(Some("turn-depth".to_string()))
        .with_parent_work_order(parent)
        .with_creator(Some(creator()));
    let err = tool
        .execute_structured(
            json!({"action": "create", "prompt": "too deep"}),
            Some(ctx_for("inv-deep-1", "turn-depth")),
        )
        .await
        .expect_err("depth enforced");
    assert!(err.to_string().contains("depth"));
    let _ = MAX_AGENT_DESCENDANTS_PER_ROOT;
    let _ = MAX_AGENT_PENDING_PER_PROJECT;
}

// ── Plan-file queue trajectory ───────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn plan_file_queue_uses_one_atomic_batch() {
    // Synthetic plan files discovered with ordinary file tools, then one
    // atomic batch call queues them sequentially and starts the first.
    let plans = [
        ("001-first.md", "First plan body"),
        ("002-second.md", "Second plan body"),
        ("003-third.md", "Third plan body"),
    ];
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-plans");
    let items: Vec<serde_json::Value> = plans
        .iter()
        .map(|(name, body)| {
            json!({"prompt": format!("Implement {name}: {body}"), "title": name.to_string()})
        })
        .collect();
    let output = tool
        .execute_structured(
            json!({
                "action": "create_batch",
                "items": items,
                "sequence": {"new_lane_label": "plan-queue", "start_first_now": true},
            }),
            Some(ctx_for("inv-plans-1", "turn-plans")),
        )
        .await
        .expect("plan queue batch")
        .output;
    let value: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(value["created"], 3);
    // Titles survive bounded; bodies do not leak into the result.
    assert!(!output.contains("First plan body"));
    let titles: Vec<String> = value["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["short_title"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        titles,
        vec!["001-first.md", "002-second.md", "003-third.md"]
    );
}

// ── List/get ─────────────────────────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn list_and_get_are_bounded_and_project_authorized() {
    let pool = test_pool().await;
    let tool = tool_for(&pool, "turn-list");
    tool.execute_structured(
        json!({"action": "create", "prompt": "list me", "title": "Listed"}),
        Some(ctx_for("inv-list-1", "turn-list")),
    )
    .await
    .expect("create");
    let listed = tool
        .execute_structured(
            json!({"action": "list", "limit": 8}),
            Some(ctx_for("inv-list-2", "turn-list")),
        )
        .await
        .expect("list")
        .output;
    assert!(listed.contains("Listed"));
    assert!(!listed.contains("list me"), "full prompt not listed");
    let value: serde_json::Value = serde_json::from_str(&listed).unwrap();
    let id = value["items"][0]["id"].as_str().unwrap().to_owned();
    let got = tool
        .execute_structured(
            json!({"action": "get", "id": id}),
            Some(ctx_for("inv-get-1", "turn-list")),
        )
        .await
        .expect("get")
        .output;
    assert!(got.contains("Listed"));
    // Foreign id reports not-found without oracling another project.
    let foreign = tool
        .execute_structured(
            json!({"action": "get", "id": "wo_doesnotexist123"}),
            Some(ctx_for("inv-get-2", "turn-list")),
        )
        .await
        .expect_err("foreign id");
    assert!(foreign.to_string().contains("not found"));
}
