//! Agent-visible project WorkOrder tool (Project Work Orders M006).
//!
//! A dedicated model-facing `work_order` tool that is semantically separate
//! from the delegated `task` (TaskTool/subagent) surface. It exposes bounded
//! single creation and atomic ordered batch creation through the canonical
//! M001 `WorkOrderService`, with lane ordering, parent lineage, authority
//! narrowing, fan-out/depth/repeat limits, invocation-scoped idempotency,
//! and compact bounded results.
//!
//! ## Design notes
//!
//! - Tool name is `work_order` (distinct from `task`). The description
//!   explicitly states project WorkOrders are durable future sessions,
//!   not delegated child runs.
//! - Project scope is bound at construction from the daemon-resolved
//!   session/project context (`SessionToolContext`). The model cannot set
//!   an arbitrary project: any `project_id`/`project_scope` input that
//!   disagrees with the bound project is rejected.
//! - Parent lineage (`parent_session_id`, `parent_turn_id`,
//!   `parent_work_order_id`) is derived host-side from the bound
//!   session/turn plus a durable session->occurrence lookup. Model-supplied
//!   parent fields are rejected as host-owned.
//! - Gates are derived host-side (immediate / delay / not_before /
//!   sequence_ready). The model never supplies raw gate JSON, trigger
//!   references, or trigger secrets. External-trigger gates are never
//!   created through this tool.
//! - Approval/sandbox ceilings come from the per-call
//!   `ToolExecutionContext` (`permission_mode`, `sandbox_profile` snapshot).
//!   Broader requests are rejected, never silently narrowed.
//! - Default model derives from the creating session's effective model
//!   (bound `parent_model` at construction), never from the human Task-mode
//!   convenience preference.
//! - `start_first_now` makes the first batch member immediate and the rest
//!   sequence-ready on the same lane. It never bypasses
//!   `WorkOrderCoordinator`; release still goes through gate evaluation.
//! - Fan-out/depth/repeat/total-bytes limits are host-owned and durable
//!   (SQL counts + parent-chain traversal), so they survive restart.
//! - Idempotency uses the model tool-call identity (`invocation_key`)
//!   namespaced with caller/project/turn scope as the M001
//!   submission/batch key. Retries converge; distinct calls never dedupe
//!   merely because prompts match; conflicting payloads under the same key
//!   surface `idempotency_conflict`.
//! - Results are bounded (`created`, `lane`, per-item
//!   `{id, position, state, short_title}`, `first_release`). Full prompts
//!   are never echoed.
//! - The tool never touches the scheduler, `AgentLoop`, sessions, jobs,
//!   worktrees, or providers directly. It only writes WorkOrder/lane/
//!   occurrence rows through `WorkOrderService`. First-release still
//!   requires the M002 coordinator wake (explicit wake in tests, timer or
//!   trigger wake in production).

use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::error::ToolError;
use crate::tool::{StructuredToolResult, Tool, ToolCategory, ToolExecutionContext};
use codegg_core::identity::{PrincipalId, ProjectId, SequenceLaneId, WorkOrderId};
use codegg_core::work_order::{
    ApprovalRequest, GateJoin, GateKind, GateSpec, LaneFailurePolicy, NewSequenceLane,
    NewWorkOrder, ReleaseGateSet, SandboxRequest, WorkOrderService, WorkspacePolicy,
    MAX_IDEMPOTENCY_KEY_LEN,
};

// ── Host-owned agent bounds (M006) ─────────────────────────────────────
// Well below the M001 human bounds (batch 32, repeat 256, lanes 64,
// members 1024) and the WorkPlan 64-item cap. Documented in the closure
// bounds table and pinned by tests.

/// Maximum items in one agent `create_batch` call.
pub const MAX_AGENT_WORK_ORDER_BATCH: usize = 16;
/// Maximum total prompt+title bytes in one agent batch.
pub const MAX_AGENT_BATCH_TOTAL_BYTES: usize = 64 * 1024;
/// Maximum per-item prompt bytes accepted through the agent tool.
/// M001 allows 32 KiB; the agent surface allows the same per item but the
/// batch total cap keeps context bounded.
pub const MAX_AGENT_PROMPT_BYTES_PER_ITEM: usize = 32 * 1024;
/// Maximum repeat count for agent-created WorkOrders (human cap is 256).
pub const MAX_AGENT_REPEAT_COUNT: u32 = 8;
/// Maximum WorkOrder lineage depth reachable through agent creation.
/// Depth 1 means a root turn with no WorkOrder parent.
pub const MAX_AGENT_WORK_ORDER_DEPTH: u64 = 4;
/// Maximum total agent-created WorkOrders attributed to one turn.
pub const MAX_AGENT_CREATED_PER_TURN: u64 = 32;
/// Maximum active/paused agent-created WorkOrders in one project.
/// Counts rows with non-null `parent_session_id` (agent lineage marker).
pub const MAX_AGENT_PENDING_PER_PROJECT: u64 = 100;
/// Maximum transitive descendants attributed to one root WorkOrder,
/// including the new batch under evaluation.
pub const MAX_AGENT_DESCENDANTS_PER_ROOT: u64 = 32;
/// Maximum short-title chars echoed in bounded results.
pub const MAX_RESULT_TITLE_CHARS: usize = 80;

// ── Tool ───────────────────────────────────────────────────────────────

/// Agent-facing project WorkOrder tool.
///
/// Bound at construction with daemon-resolved context; per-call authority
/// comes from `ToolExecutionContext`. Cloneable for registry use.
#[derive(Clone)]
pub struct WorkOrderTool {
    pool: Option<SqlitePool>,
    project_id: Option<ProjectId>,
    session_id: Option<String>,
    turn_id: Option<String>,
    parent_work_order_id: Option<WorkOrderId>,
    parent_run_id: Option<String>,
    effective_model: Option<String>,
    creator_principal: Option<PrincipalId>,
    enabled: bool,
    allowed_models: Option<std::collections::HashSet<String>>,
}

impl WorkOrderTool {
    pub fn new(pool: Option<SqlitePool>) -> Self {
        Self {
            pool,
            project_id: None,
            session_id: None,
            turn_id: None,
            parent_work_order_id: None,
            parent_run_id: None,
            effective_model: None,
            creator_principal: None,
            enabled: true,
            allowed_models: None,
        }
    }

    pub fn with_project(mut self, project: Option<ProjectId>) -> Self {
        self.project_id = project;
        self
    }

    pub fn with_session(mut self, session_id: Option<String>) -> Self {
        self.session_id = session_id;
        self
    }

    pub fn with_turn(mut self, turn_id: Option<String>) -> Self {
        self.turn_id = turn_id;
        self
    }

    pub fn with_parent_work_order(mut self, id: Option<WorkOrderId>) -> Self {
        self.parent_work_order_id = id;
        self
    }

    pub fn with_parent_run_id(mut self, run_id: Option<String>) -> Self {
        self.parent_run_id = run_id;
        self
    }

    pub fn with_turn_owner(mut self, session_id: String, turn_id: String) -> Self {
        self.session_id = Some(session_id);
        self.turn_id = Some(turn_id);
        self
    }

    pub fn with_run_owner(mut self, run_id: String, session_id: Option<String>) -> Self {
        self.parent_run_id = Some(run_id);
        if let Some(session) = session_id {
            self.session_id = Some(session);
        }
        self
    }

    pub fn with_effective_model(mut self, model: Option<String>) -> Self {
        self.effective_model = model.filter(|m| !m.trim().is_empty());
        self
    }

    pub fn with_creator(mut self, principal: Option<PrincipalId>) -> Self {
        self.creator_principal = principal;
        self
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_allowed_models(mut self, models: Option<Vec<String>>) -> Self {
        self.allowed_models = models.map(|list| list.into_iter().collect());
        self
    }

    fn service(&self) -> Result<WorkOrderService, ToolError> {
        let pool = self.pool.clone().ok_or_else(|| {
            ToolError::Execution("project work orders require a durable database pool".into())
        })?;
        Ok(WorkOrderService::with_defaults(Some(pool)))
    }

    fn bound_project(&self) -> Result<ProjectId, ToolError> {
        self.project_id.clone().ok_or_else(|| {
            ToolError::Execution("work_order tool has no bound project scope".into())
        })
    }

    fn bound_session(&self, ctx: Option<&ToolExecutionContext>) -> String {
        ctx.and_then(|c| c.session_id.clone())
            .filter(|s| !s.is_empty())
            .or_else(|| self.session_id.clone())
            .unwrap_or_default()
    }

    fn bound_turn(&self, ctx: Option<&ToolExecutionContext>) -> String {
        ctx.and_then(|c| c.turn_id.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| self.turn_id.clone())
            .unwrap_or_default()
    }

    fn creator(&self, ctx: Option<&ToolExecutionContext>) -> PrincipalId {
        if let Some(origin) = ctx
            .and_then(|c| c.origin_principal.as_deref())
            .filter(|s| !s.is_empty())
        {
            if let Ok(principal) = PrincipalId::parse(origin) {
                return principal;
            }
        }
        if let Some(creator) = self.creator_principal.clone() {
            return creator;
        }
        PrincipalId::parse("local-owner").expect("static principal parses")
    }
}

impl Default for WorkOrderTool {
    fn default() -> Self {
        Self::new(None)
    }
}

#[async_trait]
impl Tool for WorkOrderTool {
    fn name(&self) -> &str {
        "work_order"
    }

    fn description(&self) -> &str {
        "Create durable project WorkOrders (future sessions) as single items or one atomic sequential batch. Project WorkOrders enqueue future work that the release coordinator materializes into ordinary sessions; they are NOT delegated subagent runs (use task/spawn for live child execution). Supports create, create_batch (atomic ordered lane placement with optional start_first_now), list, and get. Bounded, project-scoped, and lineage-attributed."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action: create, create_batch, list, get",
                    "enum": ["create", "create_batch", "list", "get"]
                },
                "prompt": { "type": "string", "description": "Work objective for action=create (1..=32768 bytes)" },
                "title": { "type": "string", "description": "Optional bounded title (action=create, per-item in create_batch)" },
                "delay_secs": { "type": "integer", "minimum": 0, "description": "Optional delay gate for action=create" },
                "not_before_ms": { "type": "integer", "minimum": 0, "description": "Optional not-before gate (ms) for action=create" },
                "sequence_lane_id": { "type": "string", "description": "Existing lane id for sequential placement (action=create)" },
                "new_lane_label": { "type": "string", "description": "Create a new lane with this label (action=create_batch sequence, or action=create)" },
                "repeat_count": { "type": "integer", "minimum": 1, "maximum": 8, "description": "Finite repeat (agent cap 8; default 1)" },
                "requested_model": { "type": "string", "description": "Optional model override; defaults to the creating session effective model" },
                "requested_approval": { "type": "string", "description": "Optional approval snapshot: interactive, automatic, yolo (must not exceed caller ceiling)" },
                "requested_sandbox": { "type": "string", "description": "Optional sandbox snapshot: read_only, workspace_write, full_host (must not exceed caller ceiling)" },
                "workspace_policy": { "type": "string", "description": "Optional workspace policy: auto_isolated, shared, serialized" },
                "idempotency_key": { "type": "string", "description": "Optional caller key; namespaced with the tool invocation identity" },
                "items": {
                    "type": "array",
                    "maxItems": 16,
                    "description": "Ordered specs for action=create_batch. Each item has prompt, optional title, optional repeat_count.",
                    "items": { "type": "object" }
                },
                "sequence": {
                    "type": "object",
                    "description": "Optional lane placement for create_batch: {new_lane_label | existing_lane_id, start_first_now}",
                    "properties": {
                        "new_lane_label": { "type": "string" },
                        "existing_lane_id": { "type": "string" },
                        "start_first_now": { "type": "boolean" }
                    }
                },
                "limit": { "type": "integer", "minimum": 1, "maximum": 32, "description": "Bounded list page size (action=list)" },
                "state_filter": { "type": "string", "description": "Optional state filter for action=list" },
                "cursor": { "type": "string", "description": "Opaque page cursor for action=list" },
                "id": { "type": "string", "description": "WorkOrder id for action=get" }
            },
            "required": ["action"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Mutating
    }

    fn has_functional_backend(&self) -> bool {
        self.pool.is_some() && self.project_id.is_some()
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        self.execute_impl(input, None).await
    }

    async fn execute_structured(
        &self,
        input: serde_json::Value,
        ctx: Option<ToolExecutionContext>,
    ) -> Result<StructuredToolResult, ToolError> {
        let output = self.execute_impl(input, ctx.as_ref()).await?;
        Ok(StructuredToolResult::legacy(self.name(), output))
    }
}

impl WorkOrderTool {
    async fn execute_impl(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        reject_forbidden_model_fields(&input)?;
        if !self.enabled {
            return Err(ToolError::Execution(
                "agent-created persistent work orders are disabled by project policy; use human Task mode"
                    .into(),
            ));
        }
        if let Some(caller) = ctx.and_then(|c| c.caller_class.as_deref()) {
            if caller == "approval-reviewer" {
                return Err(ToolError::Execution(
                    "work_order cannot be invoked through the approval reviewer".into(),
                ));
            }
        }
        // The reviewer allowlist already excludes work_order; this is
        // defense-in-depth so a future allowlist edit cannot silently
        // enable recursive self-invocation.
        if let Some(agent) = ctx.and_then(|c| c.agent_id.as_deref()) {
            let lowered = agent.to_lowercase();
            if lowered.contains("reviewer") || lowered.contains("approval") {
                return Err(ToolError::Execution(
                    "work_order cannot be invoked through the approval reviewer".into(),
                ));
            }
        }
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("missing 'action' parameter".into()))?;
        match action {
            "create" => self.execute_create(input, ctx).await,
            "create_batch" => self.execute_batch(input, ctx).await,
            "list" => self.execute_list(input, ctx).await,
            "get" => self.execute_get(input, ctx).await,
            other => Err(ToolError::Execution(format!(
                "unsupported work_order action '{other}'"
            ))),
        }
    }

    async fn execute_create(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let project = self.bound_project()?;
        reject_project_override(&input, &project)?;
        let service = self.service()?;
        let pool = self.pool.clone().ok_or_else(|| {
            ToolError::Execution("project work orders require a durable database pool".into())
        })?;
        let session_id = self.bound_session(ctx);
        if session_id.is_empty() {
            return Err(ToolError::Execution(
                "work_order create requires a bound session scope".into(),
            ));
        }
        let turn_id = self.bound_turn(ctx);
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("missing 'prompt' parameter".into()))?;
        if prompt.len() > MAX_AGENT_PROMPT_BYTES_PER_ITEM {
            return Err(ToolError::Execution(format!(
                "prompt exceeds agent per-item bound ({MAX_AGENT_PROMPT_BYTES_PER_ITEM} bytes)"
            )));
        }
        let title = input
            .get("title")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let repeat = input
            .get("repeat_count")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v as u32)
            .unwrap_or(1);
        enforce_agent_repeat(repeat)?;
        let (requested_model, requested_approval, requested_sandbox, workspace_policy) =
            resolve_requested_policy(&input, ctx, self)?;
        let delay_secs = input.get("delay_secs").and_then(serde_json::Value::as_i64);
        let not_before_ms = input
            .get("not_before_ms")
            .and_then(serde_json::Value::as_i64);
        let lane_id_raw = input
            .get("sequence_lane_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let new_lane_label = input
            .get("new_lane_label")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if lane_id_raw.is_some() && new_lane_label.is_some() {
            return Err(ToolError::Execution(
                "set sequence_lane_id or new_lane_label, not both".into(),
            ));
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        // Optional new-lane creation is idempotent on the invocation key.
        let call_identity = resolve_call_identity(&input, ctx);
        let lane_id = if let Some(label) = new_lane_label {
            let lane_key = namespaced_key(&call_identity, "lane-new");
            let lane = service
                .create_lane(
                    &project,
                    NewSequenceLane {
                        label: (!label.trim().is_empty()).then_some(label),
                        failure_policy: LaneFailurePolicy::HoldLane,
                        idempotency_key: Some(lane_key),
                    },
                    now_ms,
                )
                .await
                .map_err(map_work_order_error)?;
            Some(lane.id)
        } else if let Some(raw) = lane_id_raw {
            let parsed = SequenceLaneId::parse(&raw)
                .map_err(|e| ToolError::Execution(format!("invalid sequence_lane_id: {e}")))?;
            // Fail closed when the lane is absent or foreign.
            let lane = service
                .get_lane(&project, &parsed)
                .await
                .map_err(map_work_order_error)?;
            if lane.is_none() {
                return Err(ToolError::Execution("work order not found".into()));
            }
            Some(parsed)
        } else {
            None
        };
        let gates = gates_for_single(delay_secs, not_before_ms, lane_id.as_ref())?;
        let parent_work_order_id = self.resolve_parent_work_order(&pool, &project).await?;
        enforce_depth(&service, &project, parent_work_order_id.as_ref()).await?;
        enforce_turn_budget(&pool, &project, &turn_id).await?;
        enforce_project_pending(&pool, &project).await?;
        if let Some(ref root) = parent_work_order_id {
            enforce_root_descendants(&pool, &project, root, 1).await?;
        }
        let submission_key = namespaced_key(&call_identity, "single");
        let creator = self.creator(ctx);
        let effective_model = requested_model.or_else(|| self.effective_model.clone());
        if let Some(ref model) = effective_model {
            enforce_model_scope(model, self.allowed_models.as_ref())?;
        }
        let outcome = service
            .create_work_order(
                &project,
                &creator,
                NewWorkOrder {
                    title,
                    prompt: prompt.to_owned(),
                    requested_model: effective_model,
                    requested_approval,
                    requested_sandbox,
                    workspace_policy,
                    gates,
                    repeat_count: repeat,
                    sequence_lane_id: lane_id.clone(),
                    parent_session_id: Some(session_id.clone()),
                    parent_turn_id: (!turn_id.is_empty()).then_some(turn_id.clone()),
                    parent_work_order_id: parent_work_order_id.clone(),
                    idempotency_key: Some(submission_key),
                },
                now_ms,
            )
            .await
            .map_err(map_work_order_error)?;
        ensure_occurrences(
            &service,
            &project,
            std::slice::from_ref(&outcome.work_order),
            now_ms,
        )
        .await?;
        let lane_state = match lane_id {
            Some(ref id) => service
                .get_lane(&project, id)
                .await
                .map_err(map_work_order_error)?
                .map(|lane| lane.id.as_str().to_owned()),
            None => outcome
                .work_order
                .sequence_lane_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
        };
        Ok(bounded_single_result(
            &outcome.work_order,
            lane_state.as_deref(),
        ))
    }

    async fn execute_batch(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let project = self.bound_project()?;
        reject_project_override(&input, &project)?;
        let service = self.service()?;
        let pool = self.pool.clone().ok_or_else(|| {
            ToolError::Execution("project work orders require a durable database pool".into())
        })?;
        let session_id = self.bound_session(ctx);
        if session_id.is_empty() {
            return Err(ToolError::Execution(
                "work_order create_batch requires a bound session scope".into(),
            ));
        }
        let turn_id = self.bound_turn(ctx);
        let items_raw = input
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ToolError::Execution("missing 'items' array".into()))?;
        if items_raw.is_empty() || items_raw.len() > MAX_AGENT_WORK_ORDER_BATCH {
            return Err(ToolError::Execution(format!(
                "batch must carry 1..={} items",
                MAX_AGENT_WORK_ORDER_BATCH
            )));
        }
        let (batch_model, batch_approval, batch_sandbox, batch_workspace) =
            resolve_requested_policy(&input, ctx, self)?;
        let effective_batch_model = batch_model.or_else(|| self.effective_model.clone());
        if let Some(ref model) = effective_batch_model {
            enforce_model_scope(model, self.allowed_models.as_ref())?;
        }
        // Sequence config: at most one of new/existing lane.
        let (new_lane_label, existing_lane_id, start_first_now) = parse_sequence(&input)?;
        if items_raw.len() == 1 && new_lane_label.is_none() && existing_lane_id.is_none() {
            // Single-item batches without lane placement behave like create.
        }
        // Total-bytes bound before touching durability.
        let mut total_bytes: usize = 0;
        let mut parsed_items: Vec<(String, Option<String>, u32)> =
            Vec::with_capacity(items_raw.len());
        for (index, raw) in items_raw.iter().enumerate() {
            let prompt = raw.get("prompt").and_then(|v| v.as_str()).ok_or_else(|| {
                ToolError::Execution(format!("items[{index}] is missing 'prompt'"))
            })?;
            if prompt.trim().is_empty() {
                return Err(ToolError::Execution(format!(
                    "items[{index}] prompt must not be empty"
                )));
            }
            if prompt.len() > MAX_AGENT_PROMPT_BYTES_PER_ITEM {
                return Err(ToolError::Execution(format!(
                    "items[{index}] prompt exceeds agent per-item bound"
                )));
            }
            let title = raw
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            let repeat = raw
                .get("repeat_count")
                .and_then(serde_json::Value::as_u64)
                .map(|v| v as u32)
                .unwrap_or(1);
            enforce_agent_repeat(repeat)
                .map_err(|e| ToolError::Execution(format!("items[{index}] {e}")))?;
            // Per-item policy overrides are not part of the narrow agent
            // surface; reject them so narrowing stays batch-uniform.
            for forbidden in [
                "requested_model",
                "requested_approval",
                "requested_sandbox",
                "workspace_policy",
                "gates",
                "gate_join",
                "trigger_ref",
                "sequence_lane_id",
            ] {
                if raw.get(forbidden).is_some() {
                    return Err(ToolError::Execution(format!(
                        "items[{index}] field '{forbidden}' is host-owned; set batch-level policy only"
                    )));
                }
            }
            total_bytes = total_bytes
                .saturating_add(prompt.len())
                .saturating_add(title.as_deref().map_or(0, |t| t.len()));
            parsed_items.push((prompt.to_owned(), title, repeat));
        }
        if total_bytes > MAX_AGENT_BATCH_TOTAL_BYTES {
            return Err(ToolError::Execution(format!(
                "batch exceeds agent total bound ({MAX_AGENT_BATCH_TOTAL_BYTES} bytes)"
            )));
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let call_identity = resolve_call_identity(&input, ctx);
        // Lane setup (idempotent on the invocation namespace).
        let lane_id = if let Some(label) = new_lane_label {
            let lane_key = namespaced_key(&call_identity, "lane-new");
            let lane = service
                .create_lane(
                    &project,
                    NewSequenceLane {
                        label: (!label.trim().is_empty()).then_some(label),
                        failure_policy: LaneFailurePolicy::HoldLane,
                        idempotency_key: Some(lane_key),
                    },
                    now_ms,
                )
                .await
                .map_err(map_work_order_error)?;
            Some(lane.id)
        } else if let Some(raw) = existing_lane_id {
            let parsed = SequenceLaneId::parse(&raw)
                .map_err(|e| ToolError::Execution(format!("invalid sequence lane id: {e}")))?;
            let lane = service
                .get_lane(&project, &parsed)
                .await
                .map_err(map_work_order_error)?;
            if lane.is_none() {
                return Err(ToolError::Execution("work order not found".into()));
            }
            Some(parsed)
        } else {
            None
        };
        let parent_work_order_id = self.resolve_parent_work_order(&pool, &project).await?;
        enforce_depth(&service, &project, parent_work_order_id.as_ref()).await?;
        enforce_turn_budget_for(&pool, &project, &turn_id, parsed_items.len() as u64).await?;
        enforce_project_pending_for(&pool, &project, parsed_items.len() as u64).await?;
        if let Some(ref root) = parent_work_order_id {
            enforce_root_descendants(&pool, &project, root, parsed_items.len() as u64).await?;
        }
        // Derive per-item gates: first immediate when start_first_now,
        // otherwise sequence_ready for every lane member. Lane-free
        // batches are independent immediate items.
        let mut new_orders: Vec<NewWorkOrder> = Vec::with_capacity(parsed_items.len());
        for (index, (prompt, title, repeat)) in parsed_items.into_iter().enumerate() {
            let gates = if let Some(ref lane) = lane_id {
                if index == 0 && start_first_now {
                    ReleaseGateSet::immediate()
                } else {
                    ReleaseGateSet {
                        join: GateJoin::All,
                        gates: vec![GateSpec {
                            kind: GateKind::SequenceReady,
                            delay_secs: None,
                            not_before_ms: None,
                            lane_id: Some(lane.clone()),
                            trigger_ref: None,
                        }],
                    }
                }
            } else {
                ReleaseGateSet::immediate()
            };
            let item_key = format!("{}#{index}", namespaced_key(&call_identity, "item"));
            let item_key = truncate_key(&item_key);
            new_orders.push(NewWorkOrder {
                title,
                prompt,
                requested_model: effective_batch_model.clone(),
                requested_approval: batch_approval,
                requested_sandbox: batch_sandbox,
                workspace_policy: batch_workspace,
                gates,
                repeat_count: repeat,
                sequence_lane_id: None,
                parent_session_id: Some(session_id.clone()),
                parent_turn_id: (!turn_id.is_empty()).then_some(turn_id.clone()),
                parent_work_order_id: parent_work_order_id.clone(),
                idempotency_key: Some(item_key),
            });
        }
        let batch_key = namespaced_key(&call_identity, "batch");
        let outcome = service
            .batch_create_work_orders(
                &project,
                &self.creator(ctx),
                new_orders,
                lane_id.clone(),
                Some(batch_key),
                now_ms,
            )
            .await
            .map_err(map_work_order_error)?;
        ensure_occurrences(&service, &project, &outcome.work_orders, now_ms).await?;
        let lane_state = match lane_id {
            Some(ref id) => service
                .get_lane(&project, id)
                .await
                .map_err(map_work_order_error)?
                .map(|lane| (lane.id.as_str().to_owned(), lane.revision)),
            None => None,
        };
        Ok(bounded_batch_result(
            &outcome.work_orders,
            lane_state,
            lane_id.is_some() && start_first_now,
        ))
    }

    async fn execute_list(
        &self,
        input: serde_json::Value,
        ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let project = self.bound_project()?;
        reject_project_override(&input, &project)?;
        let service = self.service()?;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .map(|v| v.clamp(1, 32) as u32);
        let state_filter = input
            .get("state_filter")
            .and_then(|v| v.as_str())
            .map(|raw| {
                codegg_core::work_order::WorkOrderState::parse(raw)
                    .ok_or_else(|| ToolError::Execution(format!("unknown state_filter '{raw}'")))
            })
            .transpose()?;
        let cursor = input
            .get("cursor")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let _ = ctx;
        let page = service
            .list_work_orders(&project, state_filter, cursor.as_deref(), limit)
            .await
            .map_err(map_work_order_error)?;
        let items: Vec<serde_json::Value> = page
            .work_orders
            .iter()
            .take(MAX_AGENT_WORK_ORDER_BATCH)
            .map(|wo| {
                json!({
                    "id": wo.id.as_str(),
                    "state": wo.state.as_str(),
                    "short_title": short_title(wo.title.as_deref()),
                    "sequence_lane_id": wo.sequence_lane_id.as_ref().map(|id| id.as_str()),
                })
            })
            .collect();
        Ok(json!({
            "project_scope": "current",
            "count": items.len(),
            "truncated": page.truncated,
            "next_cursor": page.next_cursor,
            "items": items,
        })
        .to_string())
    }

    async fn execute_get(
        &self,
        input: serde_json::Value,
        _ctx: Option<&ToolExecutionContext>,
    ) -> Result<String, ToolError> {
        let project = self.bound_project()?;
        reject_project_override(&input, &project)?;
        let service = self.service()?;
        let raw = input
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::Execution("missing 'id' parameter".into()))?;
        let id = WorkOrderId::parse(raw)
            .map_err(|e| ToolError::Execution(format!("invalid work order id: {e}")))?;
        let work_order = service
            .get_work_order(&project, &id)
            .await
            .map_err(map_work_order_error)?;
        match work_order {
            Some(wo) => Ok(json!({
                "id": wo.id.as_str(),
                "state": wo.state.as_str(),
                "revision": wo.revision,
                "short_title": short_title(wo.title.as_deref()),
                "prompt_preview": prompt_preview(&wo.prompt),
                "repeat_count": wo.repeat_count,
                "sequence_lane_id": wo.sequence_lane_id.as_ref().map(|id| id.as_str()),
            })
            .to_string()),
            None => Err(ToolError::Execution("work order not found".into())),
        }
    }

    async fn resolve_parent_work_order(
        &self,
        pool: &SqlitePool,
        project: &ProjectId,
    ) -> Result<Option<WorkOrderId>, ToolError> {
        if let Some(explicit) = self.parent_work_order_id.clone() {
            return Ok(Some(explicit));
        }
        let Some(session) = self.session_id.clone().filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let row: Option<(String,)> =
            sqlx::query_as("SELECT work_order_id FROM work_order_occurrence WHERE project_id = ? AND session_id = ? LIMIT 1")
                .bind(project.as_str())
                .bind(&session)
                .fetch_optional(pool)
                .await
                .map_err(|e| ToolError::Execution(format!("parent lookup failed: {e}")))?;
        match row {
            Some((work_order_id,)) => WorkOrderId::parse(&work_order_id)
                .map(Some)
                .map_err(|e| ToolError::Execution(format!("parent lookup failed: {e}"))),
            None => Ok(None),
        }
    }
}

// ── Input guards ───────────────────────────────────────────────────────

fn reject_forbidden_model_fields(input: &serde_json::Value) -> Result<(), ToolError> {
    for forbidden in [
        "parent_session_id",
        "parent_turn_id",
        "parent_work_order_id",
        "parent_run_id",
        "project_id",
        "gates",
        "gate_join",
        "trigger_ref",
        "trigger_secret",
        "trigger_verifier",
        "creator_principal",
        "origin_principal",
    ] {
        if input.get(forbidden).is_some() {
            return Err(ToolError::Execution(format!(
                "field '{forbidden}' is host-owned and cannot be set through this tool"
            )));
        }
    }
    if let Some(items) = input.get("items").and_then(|v| v.as_array()) {
        for (index, item) in items.iter().enumerate() {
            for forbidden in [
                "parent_session_id",
                "parent_turn_id",
                "parent_work_order_id",
                "project_id",
                "gates",
                "gate_join",
                "trigger_ref",
            ] {
                if item.get(forbidden).is_some() {
                    return Err(ToolError::Execution(format!(
                        "items[{index}] field '{forbidden}' is host-owned"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn reject_project_override(input: &serde_json::Value, bound: &ProjectId) -> Result<(), ToolError> {
    if let Some(scope) = input.get("project_scope").and_then(|v| v.as_str()) {
        if scope != "current" {
            return Err(ToolError::Execution(
                "cross-project target rejected: this tool is scoped to the current project".into(),
            ));
        }
    }
    if let Some(raw) = input.get("project_id").and_then(|v| v.as_str()) {
        if raw != bound.as_str() {
            return Err(ToolError::Execution(
                "cross-project target rejected: this tool is scoped to the current project".into(),
            ));
        }
    }
    Ok(())
}

fn enforce_agent_repeat(repeat: u32) -> Result<(), ToolError> {
    if repeat == 0 || repeat > MAX_AGENT_REPEAT_COUNT {
        return Err(ToolError::Execution(format!(
            "repeat_count must be 1..={} for agent-created work orders",
            MAX_AGENT_REPEAT_COUNT
        )));
    }
    Ok(())
}

fn approval_ceiling(ctx: Option<&ToolExecutionContext>) -> u8 {
    let raw = ctx.and_then(|c| c.permission_mode.as_deref()).unwrap_or("");
    // Reviewer-flavored modes never grant autonomy above interactive.
    let lowered = raw.to_lowercase();
    if lowered.contains("reviewer") {
        return 0;
    }
    match lowered.as_str() {
        "yolo" => 2,
        "automatic" | "auto" | "automatic-reviewer" => 1,
        _ => 0,
    }
}

fn sandbox_ceiling(ctx: Option<&ToolExecutionContext>) -> u8 {
    let raw = ctx.and_then(|c| c.sandbox_profile.as_deref()).unwrap_or("");
    match raw.to_lowercase().as_str() {
        "full_host" | "full-host" | "fullhost" => 2,
        "read_only" | "readonly" | "read-only" => 0,
        _ => 1,
    }
}

fn approval_rank(requested: ApprovalRequest) -> u8 {
    match requested {
        ApprovalRequest::Interactive => 0,
        ApprovalRequest::Automatic => 1,
        ApprovalRequest::Yolo => 2,
    }
}

fn sandbox_rank(requested: SandboxRequest) -> u8 {
    match requested {
        SandboxRequest::ReadOnly => 0,
        SandboxRequest::WorkspaceWrite => 1,
        SandboxRequest::FullHost => 2,
    }
}

/// Batch-uniform requested policy snapshot for one tool call.
type RequestedPolicy = (
    Option<String>,
    Option<ApprovalRequest>,
    Option<SandboxRequest>,
    Option<WorkspacePolicy>,
);

fn resolve_requested_policy(
    input: &serde_json::Value,
    ctx: Option<&ToolExecutionContext>,
    tool: &WorkOrderTool,
) -> Result<RequestedPolicy, ToolError> {
    let requested_model = input
        .get("requested_model")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    if let Some(ref model) = requested_model {
        codegg_core::work_order::validate_model(Some(model.as_str()))
            .map_err(|e| ToolError::Execution(format!("invalid requested_model: {e}")))?;
    }
    let requested_approval = input
        .get("requested_approval")
        .and_then(|v| v.as_str())
        .map(|raw| {
            ApprovalRequest::parse(raw)
                .ok_or_else(|| ToolError::Execution(format!("unknown requested_approval '{raw}'")))
        })
        .transpose()?;
    if let Some(requested) = requested_approval {
        if approval_rank(requested) > approval_ceiling(ctx) {
            return Err(ToolError::Execution(
                "requested approval exceeds the caller approval ceiling".into(),
            ));
        }
    }
    let requested_sandbox = input
        .get("requested_sandbox")
        .and_then(|v| v.as_str())
        .map(|raw| {
            SandboxRequest::parse(raw)
                .ok_or_else(|| ToolError::Execution(format!("unknown requested_sandbox '{raw}'")))
        })
        .transpose()?;
    if let Some(requested) = requested_sandbox {
        if sandbox_rank(requested) > sandbox_ceiling(ctx) {
            return Err(ToolError::Execution(
                "requested sandbox exceeds the caller sandbox ceiling".into(),
            ));
        }
    }
    let workspace_policy = input
        .get("workspace_policy")
        .and_then(|v| v.as_str())
        .map(|raw| {
            WorkspacePolicy::parse(raw)
                .ok_or_else(|| ToolError::Execution(format!("unknown workspace_policy '{raw}'")))
        })
        .transpose()?;
    let _ = tool;
    Ok((
        requested_model,
        requested_approval,
        requested_sandbox,
        workspace_policy,
    ))
}

fn enforce_model_scope(
    model: &str,
    allowed: Option<&std::collections::HashSet<String>>,
) -> Result<(), ToolError> {
    let Some(set) = allowed else {
        return Ok(());
    };
    if set.contains(model) {
        Ok(())
    } else {
        Err(ToolError::Execution(format!(
            "requested model '{model}' is not available to this project/principal"
        )))
    }
}

fn gates_for_single(
    delay_secs: Option<i64>,
    not_before_ms: Option<i64>,
    lane_id: Option<&SequenceLaneId>,
) -> Result<ReleaseGateSet, ToolError> {
    let mut gates = Vec::new();
    if let Some(lane) = lane_id {
        gates.push(GateSpec {
            kind: GateKind::SequenceReady,
            delay_secs: None,
            not_before_ms: None,
            lane_id: Some(lane.clone()),
            trigger_ref: None,
        });
    }
    if let Some(delay) = delay_secs {
        if !(0..=codegg_core::work_order::MAX_DELAY_SECS).contains(&delay) {
            return Err(ToolError::Execution(
                "delay_secs is outside supported bounds".into(),
            ));
        }
        gates.push(GateSpec {
            kind: GateKind::Delay,
            delay_secs: Some(delay),
            not_before_ms: None,
            lane_id: None,
            trigger_ref: None,
        });
    }
    if let Some(at) = not_before_ms {
        if !(0..=codegg_core::work_order::MAX_NOT_BEFORE_MS).contains(&at) {
            return Err(ToolError::Execution(
                "not_before_ms is outside supported bounds".into(),
            ));
        }
        gates.push(GateSpec {
            kind: GateKind::NotBefore,
            delay_secs: None,
            not_before_ms: Some(at),
            lane_id: None,
            trigger_ref: None,
        });
    }
    codegg_core::work_order::validate_gate_set(&gates, GateJoin::All)
        .map_err(|e| ToolError::Execution(format!("invalid release gates: {e}")))
}

fn parse_sequence(
    input: &serde_json::Value,
) -> Result<(Option<String>, Option<String>, bool), ToolError> {
    let Some(seq) = input.get("sequence") else {
        return Ok((None, None, false));
    };
    if !seq.is_object() {
        return Err(ToolError::Execution("sequence must be an object".into()));
    }
    let new_label = seq
        .get("new_lane_label")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let existing = seq
        .get("existing_lane_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    if new_label.is_some() && existing.is_some() {
        return Err(ToolError::Execution(
            "sequence sets new_lane_label or existing_lane_id, not both".into(),
        ));
    }
    let start_first_now = seq
        .get("start_first_now")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    Ok((new_label, existing, start_first_now))
}

// ── Durable bounds ─────────────────────────────────────────────────────

async fn enforce_turn_budget(
    pool: &SqlitePool,
    project: &ProjectId,
    turn_id: &str,
) -> Result<(), ToolError> {
    enforce_turn_budget_for(pool, project, turn_id, 1).await
}

async fn enforce_turn_budget_for(
    pool: &SqlitePool,
    project: &ProjectId,
    turn_id: &str,
    additional: u64,
) -> Result<(), ToolError> {
    if turn_id.is_empty() {
        return Ok(());
    }
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM work_order WHERE project_id = ? AND parent_turn_id = ?",
    )
    .bind(project.as_str())
    .bind(turn_id)
    .fetch_one(pool)
    .await
    .map_err(|e| ToolError::Execution(format!("turn budget check failed: {e}")))?;
    let existing = u64::try_from(count.0).unwrap_or(u64::MAX);
    if existing.saturating_add(additional) > MAX_AGENT_CREATED_PER_TURN {
        return Err(ToolError::Execution(format!(
            "agent turn work-order budget exhausted (max {MAX_AGENT_CREATED_PER_TURN} per turn)"
        )));
    }
    Ok(())
}

async fn enforce_project_pending(pool: &SqlitePool, project: &ProjectId) -> Result<(), ToolError> {
    enforce_project_pending_for(pool, project, 1).await
}

async fn enforce_project_pending_for(
    pool: &SqlitePool,
    project: &ProjectId,
    additional: u64,
) -> Result<(), ToolError> {
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM work_order WHERE project_id = ? AND state IN ('active','paused') AND parent_session_id IS NOT NULL",
    )
    .bind(project.as_str())
    .fetch_one(pool)
    .await
    .map_err(|e| ToolError::Execution(format!("project budget check failed: {e}")))?;
    let existing = u64::try_from(count.0).unwrap_or(u64::MAX);
    if existing.saturating_add(additional) > MAX_AGENT_PENDING_PER_PROJECT {
        return Err(ToolError::Execution(format!(
            "agent project work-order budget exhausted (max {MAX_AGENT_PENDING_PER_PROJECT} pending)"
        )));
    }
    Ok(())
}

async fn enforce_depth(
    service: &WorkOrderService,
    project: &ProjectId,
    parent: Option<&WorkOrderId>,
) -> Result<(), ToolError> {
    let Some(parent_id) = parent else {
        return Ok(());
    };
    // Depth of the new row is parent depth + 1. Walk the durable chain
    // with a hard step bound so a corrupt cycle cannot spin.
    let mut depth: u64 = 2;
    let mut current = parent_id.clone();
    for _ in 0..(MAX_AGENT_WORK_ORDER_DEPTH + 2) {
        let Some(work_order) = service
            .get_work_order(project, &current)
            .await
            .map_err(map_work_order_error)?
        else {
            return Err(ToolError::Execution(
                "parent work order is not in this project".into(),
            ));
        };
        match work_order.parent_work_order_id {
            Some(next) => {
                depth += 1;
                if depth > MAX_AGENT_WORK_ORDER_DEPTH {
                    return Err(ToolError::Execution(format!(
                        "agent work-order depth exhausted (max {MAX_AGENT_WORK_ORDER_DEPTH})"
                    )));
                }
                current = next;
            }
            None => break,
        }
    }
    if depth > MAX_AGENT_WORK_ORDER_DEPTH {
        return Err(ToolError::Execution(format!(
            "agent work-order depth exhausted (max {MAX_AGENT_WORK_ORDER_DEPTH})"
        )));
    }
    Ok(())
}

async fn enforce_root_descendants(
    pool: &SqlitePool,
    project: &ProjectId,
    root: &WorkOrderId,
    additional: u64,
) -> Result<(), ToolError> {
    let existing = count_descendants(pool, project, root).await?;
    if existing.saturating_add(additional) > MAX_AGENT_DESCENDANTS_PER_ROOT {
        return Err(ToolError::Execution(format!(
            "agent descendant budget exhausted (max {MAX_AGENT_DESCENDANTS_PER_ROOT} per root)"
        )));
    }
    Ok(())
}

async fn count_descendants(
    pool: &SqlitePool,
    project: &ProjectId,
    root: &WorkOrderId,
) -> Result<u64, ToolError> {
    // Bounded BFS over the durable parent linkage. Caps traversal well
    // above the enforced budget so counting itself is bounded.
    let mut frontier = vec![root.as_str().to_owned()];
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    seen.insert(root.as_str().to_owned());
    let mut total: u64 = 0;
    for _ in 0..8 {
        if frontier.is_empty() {
            break;
        }
        let mut next_frontier = Vec::new();
        for parent in std::mem::take(&mut frontier) {
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT id FROM work_order WHERE project_id = ? AND parent_work_order_id = ? LIMIT 128",
            )
            .bind(project.as_str())
            .bind(&parent)
            .fetch_all(pool)
            .await
            .map_err(|e| ToolError::Execution(format!("descendant check failed: {e}")))?;
            for (child,) in rows {
                if seen.insert(child.clone()) {
                    total = total.saturating_add(1);
                    if total > MAX_AGENT_DESCENDANTS_PER_ROOT + 64 {
                        return Ok(total);
                    }
                    next_frontier.push(child);
                }
            }
        }
        frontier = next_frontier;
    }
    Ok(total)
}

async fn ensure_occurrences(
    service: &WorkOrderService,
    project: &ProjectId,
    work_orders: &[codegg_core::work_order::WorkOrder],
    now_ms: i64,
) -> Result<(), ToolError> {
    for work_order in work_orders {
        let page = service
            .list_occurrences(project, &work_order.id, Some(1))
            .await
            .map_err(map_work_order_error)?;
        if page.occurrences.is_empty() {
            service
                .create_occurrence(project, &work_order.id, None, now_ms)
                .await
                .map_err(map_work_order_error)?;
        }
    }
    Ok(())
}

// ── Idempotency ────────────────────────────────────────────────────────

fn resolve_call_identity(input: &serde_json::Value, ctx: Option<&ToolExecutionContext>) -> String {
    let candidate = input["idempotency_key"]
        .as_str()
        .filter(|v| !v.is_empty())
        .map(|explicit| {
            if let Some(invocation) = ctx
                .and_then(|c| c.invocation_key.as_deref())
                .filter(|v| !v.is_empty())
            {
                format!("aw:{invocation}:{explicit}")
            } else {
                format!("aw:{explicit}")
            }
        })
        .or_else(|| {
            ctx.and_then(|c| c.invocation_key.as_deref())
                .filter(|v| !v.is_empty())
                .map(|invocation| format!("aw:{invocation}"))
        });
    let value = candidate.unwrap_or_else(|| format!("aw-legacy-{}", uuid::Uuid::new_v4().simple()));
    if value.len() <= MAX_IDEMPOTENCY_KEY_LEN {
        value
    } else {
        format!("aw-digest-{:x}", Sha256::digest(value.as_bytes()))
    }
}

fn namespaced_key(call_identity: &str, suffix: &str) -> String {
    let value = format!("{call_identity}:{suffix}");
    truncate_key(&value)
}

fn truncate_key(value: &str) -> String {
    if value.len() <= MAX_IDEMPOTENCY_KEY_LEN {
        value.to_owned()
    } else {
        format!("aw-digest-{:x}", Sha256::digest(value.as_bytes()))
    }
}

fn map_work_order_error(error: codegg_core::work_order::WorkOrderError) -> ToolError {
    use codegg_core::work_order::WorkOrderError as E;
    match &error {
        E::NotFound(_) => ToolError::Execution("work order not found".into()),
        E::RevisionConflict { .. } => ToolError::Execution(format!("conflict/stale lane: {error}")),
        E::StateConflict(_) => ToolError::Execution(format!("state conflict: {error}")),
        E::Capacity(_) => ToolError::Execution(format!("batch bound: {error}")),
        E::IdempotencyConflict(_) => ToolError::Execution(format!("idempotency conflict: {error}")),
        E::ProjectMismatch(_) => {
            ToolError::Execution(format!("authorization/project ceiling: {error}"))
        }
        E::Invalid { .. } | E::PromptTooLarge { .. } => {
            ToolError::Execution(format!("validation: {error}"))
        }
        E::Unavailable(_) | E::Storage(_) => {
            ToolError::Execution(format!("service unavailable: {error}"))
        }
    }
}

// ── Bounded results ────────────────────────────────────────────────────

fn short_title(title: Option<&str>) -> String {
    let raw = title.unwrap_or("").trim();
    if raw.is_empty() {
        return String::new();
    }
    let mut out: String = raw.chars().take(MAX_RESULT_TITLE_CHARS).collect();
    if raw.chars().count() > MAX_RESULT_TITLE_CHARS {
        out.push('…');
    }
    out
}

fn prompt_preview(prompt: &str) -> String {
    let flat: String = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = flat.chars().take(240).collect();
    if flat.chars().count() > 240 {
        out.push('…');
    }
    out
}

fn bounded_single_result(
    work_order: &codegg_core::work_order::WorkOrder,
    lane: Option<&str>,
) -> String {
    json!({
        "created": 1,
        "lane": lane,
        "items": [{
            "id": work_order.id.as_str(),
            "position": 0,
            "state": work_order.state.as_str(),
            "short_title": short_title(work_order.title.as_deref()),
        }],
        "first_release": "immediate",
    })
    .to_string()
}

fn bounded_batch_result(
    work_orders: &[codegg_core::work_order::WorkOrder],
    lane: Option<(String, u64)>,
    start_first_now: bool,
) -> String {
    let items: Vec<serde_json::Value> = work_orders
        .iter()
        .enumerate()
        .take(MAX_AGENT_WORK_ORDER_BATCH)
        .map(|(position, wo)| {
            json!({
                "id": wo.id.as_str(),
                "position": position,
                "state": wo.state.as_str(),
                "short_title": short_title(wo.title.as_deref()),
            })
        })
        .collect();
    json!({
        "created": work_orders.len(),
        "lane": lane.as_ref().map(|(id, _)| id),
        "lane_revision": lane.as_ref().map(|(_, rev)| rev),
        "items": items,
        "first_release": if start_first_now { "immediate" } else { "sequence" },
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolBackendKind;

    fn test_context(invocation: &str) -> ToolExecutionContext {
        let mut ctx = ToolExecutionContext::with_backend(ToolBackendKind::Native);
        ctx.session_id = Some("session-1".to_string());
        ctx.turn_id = Some("turn-1".to_string());
        ctx.invocation_key = Some(invocation.to_string());
        ctx.permission_mode = Some("interactive".to_string());
        ctx.sandbox_profile = Some("workspace_write".to_string());
        ctx.caller_class = Some("agent".to_string());
        ctx
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn agent_repeat_cap_is_below_human_cap() {
        assert!(MAX_AGENT_REPEAT_COUNT < codegg_core::work_order::MAX_REPEAT_COUNT);
        assert!(MAX_AGENT_WORK_ORDER_BATCH < codegg_core::work_order::MAX_WORK_ORDER_BATCH_ITEMS);
        assert!(MAX_AGENT_WORK_ORDER_BATCH < 64);
    }

    #[test]
    fn forbidden_parent_fields_are_rejected() {
        let input = json!({"action": "create", "prompt": "x", "parent_session_id": "s"});
        assert!(reject_forbidden_model_fields(&input).is_err());
        let batch = json!({"action": "create_batch", "items": [{"prompt": "x", "gates": []}]});
        assert!(reject_forbidden_model_fields(&batch).is_err());
    }

    #[test]
    fn cross_project_override_is_rejected() {
        let project = ProjectId::parse("project-1").unwrap();
        let input = json!({"action": "list", "project_scope": "other"});
        assert!(reject_project_override(&input, &project).is_err());
    }

    #[test]
    fn broader_approval_is_rejected() {
        let ctx = test_context("inv-1");
        let input = json!({"action": "create", "prompt": "x", "requested_approval": "yolo"});
        let tool = WorkOrderTool::new(None);
        assert!(resolve_requested_policy(&input, Some(&ctx), &tool).is_err());
    }

    #[test]
    fn broader_sandbox_is_rejected() {
        let ctx = test_context("inv-1");
        let input = json!({"action": "create", "prompt": "x", "requested_sandbox": "full_host"});
        let tool = WorkOrderTool::new(None);
        assert!(resolve_requested_policy(&input, Some(&ctx), &tool).is_err());
    }

    #[test]
    fn reviewer_caller_is_rejected() {
        let mut ctx = test_context("inv-1");
        ctx.caller_class = Some("approval-reviewer".to_string());
        let tool = WorkOrderTool::new(None).with_project(ProjectId::parse("project-1").ok());
        let input = json!({"action": "list"});
        let output = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(tool.execute_impl(input, Some(&ctx)));
        assert!(output.is_err());
    }

    #[test]
    fn call_identity_namespaces_explicit_keys() {
        let ctx = test_context("inv-abc");
        let input = json!({"idempotency_key": "k1"});
        let first = resolve_call_identity(&input, Some(&ctx));
        let other_ctx = test_context("inv-other");
        let second = resolve_call_identity(&input, Some(&other_ctx));
        assert_ne!(first, second);
        assert!(first.len() <= MAX_IDEMPOTENCY_KEY_LEN);
    }
}
