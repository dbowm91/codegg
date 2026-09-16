//! WorkPlan-to-Todo one-way sync helpers (long-horizon M003).
//!
//! WorkPlan selects current/actionable items; TodoState projects a small
//! subset per the resolved `TaskStatePolicy`. Permitted Todo status changes
//! translate back only on exact identity/revision mapping, and a Todo
//! `completed` flag alone never satisfies host-only acceptance.

use codegg_core::task_state::TodoItem;
use codegg_core::work_plan::{
    parse_todo_work_ref, validate_todo_feedback, WorkPlanEvidenceSnapshot, WorkPlanStore,
};
use sqlx::SqlitePool;

/// Attempt to translate Todo status changes back into validated WorkPlan
/// updates.
///
/// Best-effort and bounded: stale mappings, unmapped free-form todos, owned
/// child items (which require the explicit WorkPlan tool with caller
/// authority), invalid transitions, and host-evidence-gated completions are
/// skipped without failing the Todo write. Returns the number of WorkPlan
/// items updated.
pub async fn try_translate_todo_feedback(
    pool: &SqlitePool,
    session_id: &str,
    todos: &[TodoItem],
) -> usize {
    let store = WorkPlanStore::new(pool.clone());
    let plan = match store.active_for_session(session_id).await {
        Ok(Some(plan)) => plan,
        _ => return 0,
    };
    let items = match store.list_items(&plan.id).await {
        Ok(items) => items,
        Err(_) => return 0,
    };
    let evidence = crate::work_plan_evidence::assemble(pool, &items)
        .await
        .unwrap_or_else(|_| WorkPlanEvidenceSnapshot::empty());

    let mut updated = 0usize;
    for todo in todos.iter().take(12) {
        let Some((work_id, _)) = parse_todo_work_ref(todo.id.as_str()) else {
            continue;
        };
        let Some(current) = items.iter().find(|item| item.id == work_id) else {
            continue;
        };
        // Child-owned items require the explicit WorkPlan tool path with
        // caller-run authority; Todo feedback never crosses that boundary.
        if current.owner_run_id.is_some() || current.owner_job_id.is_some() {
            continue;
        }
        let target = match validate_todo_feedback(todo, current, &evidence) {
            Ok(target) => target,
            Err(_) => continue,
        };
        // Todo feedback carries status plus an optional blocker reason. The
        // blocker for a `Blocked` Todo comes from the Todo item; other
        // transitions preserve the current next-action.
        if target == current.status {
            continue;
        }
        let blocker = if target == codegg_core::work_plan::WorkItemStatus::Blocked {
            todo.blocker.clone().or_else(|| current.blocker.clone())
        } else {
            None
        };
        let result = store
            .transition_item(
                &current.id,
                current.revision,
                target,
                blocker,
                current.next_action.clone(),
            )
            .await;
        if result.is_ok() {
            updated += 1;
        }
    }
    updated
}

/// Project the active WorkPlan into the durable session Todo store.
///
/// Called after host-owned WorkPlan mutations so restart reconstructs the
/// same projection from durable revisions. Respects the resolved policy cap;
/// never erases plan history (terminal items are not projected).
pub async fn project_plan_to_session_todos(
    pool: &SqlitePool,
    session_id: &str,
    policy: &codegg_core::model_profile::types::TaskStatePolicy,
) -> Result<usize, String> {
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .active_for_session(session_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(plan) = plan else {
        return Ok(0);
    };
    let items = store
        .list_items(&plan.id)
        .await
        .map_err(|error| error.to_string())?;
    let projected = codegg_core::work_plan::project_to_todo_items(&plan, &items, policy);
    let inputs: Vec<codegg_core::session::models::TodoItemInput> = projected
        .iter()
        .map(|item| codegg_core::session::models::TodoItemInput {
            content: format!("[{}] {}", item.id, item.content),
            status: item.status_str().to_string(),
            priority: item.priority_str().to_string(),
        })
        .collect();
    let count = inputs.len();
    codegg_core::session::store::TodoStore::new(pool.clone())
        .set(session_id, inputs)
        .await
        .map_err(|error| error.to_string())?;
    Ok(count)
}
