//! Bounded model-facing WorkPlan tools (long-horizon M003).
//!
//! `work_plan_get` is the compact read surface: it defaults to the
//! current/actionable slice and supports bounded pagination/lookup for larger
//! plans. `work_plan_update_item` is the narrow mutation surface: it requires
//! the expected item revision, validates status/dependency transitions,
//! permits progress notes (`next_action`) and blocker updates, and forbids
//! direct fabrication of host evidence, immutable objective provenance, and
//! dependency rewrites. Stale writers receive explicit conflicts.

use async_trait::async_trait;
use sqlx::SqlitePool;

use crate::error::ToolError;
use crate::tool::{Tool, ToolCategory};
use codegg_core::work_plan::{
    assess_work_plan, lookup_item_summary, project_work_plan, WorkItemId, WorkItemStatus,
    WorkPlanId, WorkPlanProjectionParams, WorkPlanStore,
};

fn parse_status(value: &str) -> Option<WorkItemStatus> {
    WorkItemStatus::parse(value)
}

fn work_plan_snapshot(
    plan: &codegg_core::work_plan::WorkPlan,
    actionable_count: usize,
    blocked_count: usize,
    in_progress_count: usize,
    assessment: &codegg_core::work_plan::WorkPlanCompletionAssessment,
) -> codegg_core::bus::events::WorkPlanSnapshot {
    let objective_preview: String = plan.objective.chars().take(200).collect();
    codegg_core::bus::events::WorkPlanSnapshot {
        plan_id: plan.id.as_str().to_string(),
        revision: plan.revision,
        status: plan.status.as_str().to_string(),
        objective_preview,
        current_phase: plan.current_phase.clone(),
        current_item_id: plan
            .current_item_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
        total_items: 0,
        actionable_count,
        blocked_count,
        in_progress_count,
        assessment: assessment.reason_code().to_string(),
    }
}

fn publish_work_plan(
    session_id: &str,
    plan: &codegg_core::work_plan::WorkPlan,
    items: &[codegg_core::work_plan::WorkItem],
    assessment: &codegg_core::work_plan::WorkPlanCompletionAssessment,
) {
    use codegg_core::work_plan::WorkItemStatus as ItemStatus;
    let actionable = items
        .iter()
        .filter(|item| matches!(item.status, ItemStatus::Pending | ItemStatus::Actionable))
        .count();
    let mut snapshot = work_plan_snapshot(
        plan,
        actionable,
        items
            .iter()
            .filter(|item| item.status == ItemStatus::Blocked)
            .count(),
        items
            .iter()
            .filter(|item| item.status == ItemStatus::InProgress)
            .count(),
        assessment,
    );
    snapshot.total_items = items.len();
    crate::bus::global::GlobalEventBus::publish(crate::bus::events::AppEvent::WorkPlanUpdated {
        session_id: session_id.to_string(),
        plan: snapshot,
    });
}

pub struct WorkPlanGetTool {
    pool: SqlitePool,
    session_id: String,
}

impl WorkPlanGetTool {
    pub fn new(pool: SqlitePool, session_id: String) -> Self {
        Self { pool, session_id }
    }
}

#[async_trait]
impl Tool for WorkPlanGetTool {
    fn name(&self) -> &str {
        "work_plan_get"
    }

    fn description(&self) -> &str {
        "Read the durable WorkPlan projection: current/actionable items by default with bounded pagination"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "plan_id": { "type": "string", "description": "Optional plan id (defaults to the active session plan)" },
                "item_id": { "type": "string", "description": "Optional single item lookup (wi_ id)" },
                "offset": { "type": "integer", "minimum": 0, "maximum": 64 },
                "limit": { "type": "integer", "minimum": 1, "maximum": 8 },
                "include_completed": { "type": "boolean" }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadOnly
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let store = WorkPlanStore::new(self.pool.clone());
        let plan = if let Some(plan_id) = input.get("plan_id").and_then(|value| value.as_str()) {
            let id = WorkPlanId(plan_id.to_string());
            store
                .get(&id)
                .await
                .map_err(|error| ToolError::Execution(error.to_string()))?
                .ok_or_else(|| ToolError::Execution("work plan not found".to_string()))?
        } else {
            store
                .active_for_session(&self.session_id)
                .await
                .map_err(|error| ToolError::Execution(error.to_string()))?
                .ok_or_else(|| {
                    ToolError::Execution("no active work plan for this session".to_string())
                })?
        };
        if plan.session_id != self.session_id {
            return Err(ToolError::Execution(
                "work plan belongs to a different session".to_string(),
            ));
        }
        let items = store
            .list_items(&plan.id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;

        if let Some(item_id) = input.get("item_id").and_then(|value| value.as_str()) {
            let lookup = WorkItemId(item_id.to_string());
            let summary = lookup_item_summary(&items, &lookup)
                .ok_or_else(|| ToolError::Execution("work item not found".to_string()))?;
            let evidence = crate::work_plan_evidence::assemble(&self.pool, &items)
                .await
                .unwrap_or_default();
            let assessment = assess_work_plan(&plan, &items, &evidence);
            return Ok(serde_json::json!({
                "plan_id": plan.id.as_str(),
                "plan_revision": plan.revision,
                "plan_status": plan.status.as_str(),
                "assessment": assessment.reason_code(),
                "item": summary,
            })
            .to_string());
        }

        let offset = input
            .get("offset")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
            .min(64) as usize;
        let limit = input
            .get("limit")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(5)
            .clamp(1, 8) as usize;
        let include_completed = input
            .get("include_completed")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let params = WorkPlanProjectionParams {
            offset,
            limit,
            include_completed,
        };
        let projection = project_work_plan(&plan, &items, &params);
        let evidence = crate::work_plan_evidence::assemble(&self.pool, &items)
            .await
            .unwrap_or_default();
        let assessment = assess_work_plan(&plan, &items, &evidence);
        Ok(serde_json::json!({
            "plan_id": projection.plan_id.as_str(),
            "plan_revision": projection.revision,
            "plan_status": projection.status.as_str(),
            "objective_preview": projection.objective_preview,
            "current_phase": projection.current_phase,
            "current_item_id": projection.current_item_id.as_ref().map(|id| id.as_str()),
            "total_items": projection.total_items,
            "completed_items": projection.completed_items,
            "actionable_count": projection.actionable_count,
            "blocked_count": projection.blocked_count,
            "in_progress_count": projection.in_progress_count,
            "assessment": assessment.reason_code(),
            "items": projection.items,
            "truncated": projection.truncated,
        })
        .to_string())
    }
}

pub struct WorkPlanUpdateItemTool {
    pool: SqlitePool,
    session_id: String,
}

impl WorkPlanUpdateItemTool {
    pub fn new(pool: SqlitePool, session_id: String) -> Self {
        Self { pool, session_id }
    }
}

#[async_trait]
impl Tool for WorkPlanUpdateItemTool {
    fn name(&self) -> &str {
        "work_plan_update_item"
    }

    fn description(&self) -> &str {
        "Update one WorkPlan item: status/blocker/next-action with revision checking (no host-evidence fabrication)"
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "required": ["item_id", "expected_revision"],
            "properties": {
                "item_id": { "type": "string", "description": "Work item id (wi_ prefix)" },
                "expected_revision": { "type": "integer", "minimum": 0 },
                "status": { "type": "string", "enum": ["pending", "actionable", "in_progress", "blocked", "completed", "cancelled"] },
                "blocker": { "type": ["string", "null"] },
                "next_action": { "type": ["string", "null"] },
                "caller_run_id": { "type": "string", "description": "Optional delegated-run identity for child reporting" }
            }
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::SafeMutating
    }

    async fn execute(&self, input: serde_json::Value) -> Result<String, ToolError> {
        let item_id_str = input
            .get("item_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| ToolError::Execution("missing 'item_id'".to_string()))?;
        let expected_revision = input
            .get("expected_revision")
            .and_then(serde_json::Value::as_i64)
            .ok_or_else(|| ToolError::Execution("missing 'expected_revision'".to_string()))?;
        let item_id = WorkItemId(item_id_str.to_string());
        if !item_id_str.starts_with("wi_") {
            return Err(ToolError::Execution(
                "work item id must carry wi_ prefix".to_string(),
            ));
        }

        // Reject forbidden model-mutated fields loudly rather than silently
        // ignoring them: dependencies, acceptance, evidence, objective, and
        // owner refs are host-owned.
        for forbidden in [
            "dependencies",
            "parent_item_id",
            "acceptance",
            "evidence",
            "objective",
            "origin_provenance",
            "owner_run_id",
            "owner_job_id",
        ] {
            if input.get(forbidden).is_some() {
                return Err(ToolError::Execution(format!(
                    "field '{forbidden}' is host-owned and cannot be set through this tool"
                )));
            }
        }

        let store = WorkPlanStore::new(self.pool.clone());
        let current = store
            .get_item(&item_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| ToolError::Execution("work item not found".to_string()))?;
        let plan = store
            .get(&current.plan_id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?
            .ok_or_else(|| ToolError::Execution("owning work plan not found".to_string()))?;
        if plan.session_id != self.session_id {
            return Err(ToolError::Execution(
                "work plan belongs to a different session".to_string(),
            ));
        }
        if current.revision != expected_revision {
            return Err(ToolError::Execution(format!(
                "stale work item revision: expected {expected_revision}, found {}",
                current.revision
            )));
        }
        // Child reporting authority: an item assigned to a delegated run can
        // only be reported against by that same run. Unassigned items accept
        // session-owner updates; callers without a run id cannot touch
        // assigned items.
        let caller_run_id = input.get("caller_run_id").and_then(|value| value.as_str());
        if let Some(owner) = current.owner_run_id.as_deref() {
            match caller_run_id {
                Some(caller) if caller == owner => {}
                Some(_) => {
                    return Err(ToolError::Execution(
                        "child cannot update an item assigned to a different run".to_string(),
                    ));
                }
                None => {
                    return Err(ToolError::Execution(
                        "this item is assigned to a delegated run; caller_run_id is required"
                            .to_string(),
                    ));
                }
            }
        }

        let blocker = input
            .get("blocker")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        let next_action = input
            .get("next_action")
            .and_then(|value| value.as_str())
            .map(str::to_string);
        if let Some(action) = next_action.as_deref() {
            if action.chars().count() > 1024 || action.contains('\0') {
                return Err(ToolError::Execution(
                    "next_action exceeds bound".to_string(),
                ));
            }
        }

        if let Some(status_str) = input.get("status").and_then(|value| value.as_str()) {
            let new_status = parse_status(status_str)
                .ok_or_else(|| ToolError::Execution(format!("invalid status: {status_str}")))?;
            // Host-evidence gate for completion: a model assertion alone can
            // never satisfy host-only acceptance. Resolve canonical evidence
            // first; dangling refs stay unavailable.
            if new_status == WorkItemStatus::Completed {
                let items = store
                    .list_items(&plan.id)
                    .await
                    .map_err(|error| ToolError::Execution(error.to_string()))?;
                let evidence = crate::work_plan_evidence::assemble(&self.pool, &items)
                    .await
                    .unwrap_or_default();
                let Some(latest) = items.iter().find(|item| item.id == item_id) else {
                    return Err(ToolError::Execution("work item not found".to_string()));
                };
                if !codegg_core::work_plan::item_is_satisfied(latest, &evidence) {
                    return Err(ToolError::Execution(
                        "host-owned evidence is required before completing this item".to_string(),
                    ));
                }
            }
            let (updated_plan, updated_item) = store
                .transition_item(
                    &item_id,
                    expected_revision,
                    new_status,
                    blocker,
                    next_action,
                )
                .await
                .map_err(|error| match error {
                    codegg_core::work_plan::WorkPlanError::Conflict { expected, found } => {
                        ToolError::Execution(format!(
                            "stale work item revision: expected {expected}, found {found}"
                        ))
                    }
                    other => ToolError::Execution(other.to_string()),
                })?;
            let items = store
                .list_items(&updated_plan.id)
                .await
                .map_err(|error| ToolError::Execution(error.to_string()))?;
            let evidence = crate::work_plan_evidence::assemble(&self.pool, &items)
                .await
                .unwrap_or_default();
            let assessment = assess_work_plan(&updated_plan, &items, &evidence);
            publish_work_plan(&self.session_id, &updated_plan, &items, &assessment);
            // Keep the durable Todo projection in sync without erasing plan
            // history: terminal items stay out of Todo context.
            if let Ok(policy) = todo_policy_for_session() {
                let _ = crate::work_plan_todo_sync::project_plan_to_session_todos(
                    &self.pool,
                    &self.session_id,
                    &policy,
                )
                .await;
            }
            return Ok(serde_json::json!({
                "plan_id": updated_plan.id.as_str(),
                "plan_revision": updated_plan.revision,
                "item_id": updated_item.id.as_str(),
                "item_revision": updated_item.revision,
                "item_status": updated_item.status.as_str(),
                "assessment": assessment.reason_code(),
            })
            .to_string());
        }

        // Progress-note path: blocker/next-action only, no status change.
        if blocker.is_none() && next_action.is_none() {
            return Err(ToolError::Execution(
                "nothing to update: provide status, blocker, or next_action".to_string(),
            ));
        }
        let patch = codegg_core::work_plan::model::WorkItemPatch {
            blocker: blocker.map(Some),
            next_action: next_action.map(Some),
            ..Default::default()
        };
        let (updated_plan, updated_item) = store
            .update_item(&item_id, expected_revision, patch)
            .await
            .map_err(|error| match error {
                codegg_core::work_plan::WorkPlanError::Conflict { expected, found } => {
                    ToolError::Execution(format!(
                        "stale work item revision: expected {expected}, found {found}"
                    ))
                }
                other => ToolError::Execution(other.to_string()),
            })?;
        let items = store
            .list_items(&updated_plan.id)
            .await
            .map_err(|error| ToolError::Execution(error.to_string()))?;
        let evidence = crate::work_plan_evidence::assemble(&self.pool, &items)
            .await
            .unwrap_or_default();
        let assessment = assess_work_plan(&updated_plan, &items, &evidence);
        publish_work_plan(&self.session_id, &updated_plan, &items, &assessment);
        Ok(serde_json::json!({
            "plan_id": updated_plan.id.as_str(),
            "plan_revision": updated_plan.revision,
            "item_id": updated_item.id.as_str(),
            "item_revision": updated_item.revision,
            "item_status": updated_item.status.as_str(),
            "assessment": assessment.reason_code(),
        })
        .to_string())
    }
}

fn todo_policy_for_session() -> Result<codegg_core::model_profile::types::TaskStatePolicy, String> {
    Ok(codegg_core::model_profile::types::TaskStatePolicy::explicit_todo())
}
