//! One-way WorkPlan-to-Todo projection contract (long-horizon M003).
//!
//! WorkPlan is the authoritative detailed plan state; TodoState is a bounded
//! projection. This module maps the current/actionable slice into Todo items
//! respecting the resolved [`TaskStatePolicy`], and validates the narrow
//! feedback path from Todo status changes back into WorkPlan updates.
//!
//! Completed host-only acceptance is never inferred from a Todo `completed`
//! flag alone: feedback that would complete an item without host satisfaction
//! is rejected with [`TodoFeedbackError::HostEvidenceRequired`].

use thiserror::Error;

use super::evidence::{item_is_satisfied, WorkPlanEvidenceSnapshot};
use super::model::{
    actionable_items, can_transition_item, WorkItem, WorkItemId, WorkItemStatus, WorkPlan,
};
use crate::model_profile::types::{TaskStatePolicy, TodoMode};
use crate::task_state::{TodoItem, TodoPriority, TodoStatus};

/// Separator between the WorkItem id and its revision in a projected Todo id.
/// Example: `wi_abc123@r3`.
pub const TODO_WORK_REF_SEPARATOR: &str = "@r";

/// Maximum Todo content chars for a projected item. WorkItem descriptions
/// are already bounded (1024); the Todo projection keeps a shorter excerpt
/// so the model context stays compact.
pub const MAX_PROJECTED_TODO_CONTENT_CHARS: usize = 256;

#[derive(Debug, Error)]
pub enum TodoFeedbackError {
    #[error("todo does not carry a work item reference")]
    NotMapped,
    #[error("todo maps to a different work item")]
    IdentityMismatch,
    #[error("stale todo mapping: expected revision {expected}, found {found}")]
    Stale { expected: i64, found: i64 },
    #[error("todo status transition is not a valid work item transition")]
    InvalidTransition,
    #[error("host-owned evidence is required before completing this item")]
    HostEvidenceRequired,
}

/// Projected Todo id carrying exact identity and revision.
///
/// The revision pins the mapping: feedback that presents a stale revision
/// fails with [`TodoFeedbackError::Stale`] instead of silently reapplying to
/// newer plan state.
pub fn todo_id_for_work_item(item: &WorkItem) -> String {
    format!(
        "{}{}{}",
        item.id.as_str(),
        TODO_WORK_REF_SEPARATOR,
        item.revision
    )
}

/// Parse a projected Todo id back into `(WorkItemId, expected_revision)`.
pub fn parse_todo_work_ref(todo_id: &str) -> Option<(WorkItemId, i64)> {
    let (id_part, rev_part) = todo_id.rsplit_once(TODO_WORK_REF_SEPARATOR)?;
    if id_part.is_empty() || rev_part.is_empty() {
        return None;
    }
    if !id_part.starts_with("wi_") {
        return None;
    }
    let revision: i64 = rev_part.parse().ok()?;
    if revision < 0 {
        return None;
    }
    Some((WorkItemId(id_part.to_string()), revision))
}

fn bounded_content(value: &str) -> String {
    value
        .chars()
        .take(MAX_PROJECTED_TODO_CONTENT_CHARS)
        .collect()
}

fn work_status_to_todo(status: WorkItemStatus, is_current: bool) -> TodoStatus {
    match status {
        WorkItemStatus::Pending | WorkItemStatus::Actionable => {
            if is_current {
                TodoStatus::InProgress
            } else {
                TodoStatus::Pending
            }
        }
        WorkItemStatus::InProgress => TodoStatus::InProgress,
        WorkItemStatus::Blocked => TodoStatus::Blocked,
        WorkItemStatus::Completed => TodoStatus::Completed,
        WorkItemStatus::Cancelled => TodoStatus::Cancelled,
    }
}

fn todo_status_to_work(status: TodoStatus) -> WorkItemStatus {
    match status {
        TodoStatus::Pending => WorkItemStatus::Pending,
        TodoStatus::InProgress => WorkItemStatus::InProgress,
        TodoStatus::Blocked => WorkItemStatus::Blocked,
        TodoStatus::Completed => WorkItemStatus::Completed,
        TodoStatus::Cancelled => WorkItemStatus::Cancelled,
    }
}

/// Project the current/actionable slice into Todo items.
///
/// - `Disabled` mode yields no items.
/// - The plan's `current_item_id` (when non-terminal) leads, followed by
///   actionable items in stable order, truncated to `policy.max_total_items`.
/// - At most one item maps to `InProgress` so `require_single_in_progress`
///   policies stay valid.
/// - Terminal items are never projected: history stays in the durable plan,
///   not in Todo context.
pub fn project_to_todo_items(
    plan: &WorkPlan,
    items: &[WorkItem],
    policy: &TaskStatePolicy,
) -> Vec<TodoItem> {
    if policy.mode == TodoMode::Disabled {
        return Vec::new();
    }
    if policy.max_total_items == 0 {
        return Vec::new();
    }
    let cap = policy.max_total_items.clamp(1, 12);
    let actionable = actionable_items(items);

    let mut ordered: Vec<&WorkItem> = Vec::new();
    if let Some(current_id) = plan.current_item_id.as_ref() {
        if let Some(current) = items.iter().find(|item| item.id == *current_id) {
            if !current.status.is_terminal() {
                ordered.push(current);
            }
        }
    }
    for item in &actionable {
        if ordered.len() >= cap {
            break;
        }
        if !ordered.iter().any(|existing| existing.id == item.id) {
            ordered.push(item);
        }
    }
    // Fill remaining budget with in-progress remainder so a live item is
    // still visible when nothing is actionable.
    if ordered.len() < cap {
        let mut seen: std::collections::HashSet<&str> =
            ordered.iter().map(|item| item.id.as_str()).collect();
        for item in items
            .iter()
            .filter(|item| item.status == WorkItemStatus::InProgress)
        {
            if seen.insert(item.id.as_str()) {
                ordered.push(item);
                if ordered.len() >= cap {
                    break;
                }
            }
        }
    }

    let current_id = ordered.first().map(|item| item.id.as_str());
    ordered
        .into_iter()
        .map(|item| {
            let is_current = Some(item.id.as_str()) == current_id;
            TodoItem {
                id: todo_id_for_work_item(item),
                content: bounded_content(item.description.trim()),
                status: work_status_to_todo(item.status, is_current),
                priority: if is_current {
                    TodoPriority::High
                } else {
                    TodoPriority::Medium
                },
                blocker: if item.status == WorkItemStatus::Blocked {
                    item.blocker.clone()
                } else {
                    None
                },
            }
        })
        .collect()
}

/// Validate a Todo status change against the exact mapped WorkItem.
///
/// The caller supplies the current WorkItem (loaded by the parsed identity)
/// and the host-evidence snapshot. Success returns the target
/// [`WorkItemStatus`] the caller may apply through the revision-checked
/// store path. Failure never mutates plan state.
pub fn validate_todo_feedback(
    todo: &TodoItem,
    current: &WorkItem,
    evidence: &WorkPlanEvidenceSnapshot,
) -> Result<WorkItemStatus, TodoFeedbackError> {
    let (mapped_id, expected_revision) =
        parse_todo_work_ref(todo.id.as_str()).ok_or(TodoFeedbackError::NotMapped)?;
    if mapped_id != current.id {
        return Err(TodoFeedbackError::IdentityMismatch);
    }
    if expected_revision != current.revision {
        return Err(TodoFeedbackError::Stale {
            expected: expected_revision,
            found: current.revision,
        });
    }
    if current.status.is_terminal() {
        return Err(TodoFeedbackError::InvalidTransition);
    }
    let target = todo_status_to_work(todo.status);
    if !can_transition_item(current.status, target) {
        return Err(TodoFeedbackError::InvalidTransition);
    }
    if target == WorkItemStatus::Completed && !item_is_satisfied(current, evidence) {
        return Err(TodoFeedbackError::HostEvidenceRequired);
    }
    // A Todo `blocked` flag without a blocker reason cannot fabricate plan
    // blocker state; the caller must supply the reason through the WorkPlan
    // tool path.
    if target == WorkItemStatus::Blocked
        && todo.blocker.as_deref().unwrap_or("").trim().is_empty()
        && current.blocker.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err(TodoFeedbackError::InvalidTransition);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_profile::types::TaskStatePolicy;
    use crate::work_plan::model::{
        WorkAcceptance, WorkAcceptanceDisposition, WorkPlanId, WorkPlanStatus,
    };
    use chrono::Utc;

    fn plan_fixture() -> WorkPlan {
        let now = Utc::now();
        WorkPlan {
            id: WorkPlanId("wp_1".to_string()),
            revision: 0,
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "o".to_string(),
            objective_digest: "sha256:x".to_string(),
            origin_provenance: "turn:t".to_string(),
            status: WorkPlanStatus::Active,
            current_phase: None,
            current_item_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    fn item_fixture(id: &str, status: WorkItemStatus, position: i64, revision: i64) -> WorkItem {
        let now = Utc::now();
        WorkItem {
            id: WorkItemId(format!("wi_{id}")),
            plan_id: WorkPlanId("wp_1".to_string()),
            revision,
            position,
            parent_item_id: None,
            dependencies: vec![],
            status,
            description: format!("work {id}"),
            acceptance: vec![WorkAcceptance {
                description: "c".to_string(),
                disposition: WorkAcceptanceDisposition::Unmet,
                note: None,
            }],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn todo_id_carries_exact_revision() {
        let item = item_fixture("a", WorkItemStatus::Pending, 0, 3);
        let id = todo_id_for_work_item(&item);
        let (parsed_id, revision) = parse_todo_work_ref(&id).unwrap();
        assert_eq!(parsed_id, item.id);
        assert_eq!(revision, 3);
        assert!(parse_todo_work_ref("arbitrary-uuid").is_none());
        assert!(parse_todo_work_ref("wi_a").is_none());
    }

    #[test]
    fn projection_respects_all_todo_modes() {
        let plan = plan_fixture();
        let items: Vec<WorkItem> = (0..20)
            .map(|index| item_fixture(&format!("item{index}"), WorkItemStatus::Pending, index, 0))
            .collect();

        let disabled = project_to_todo_items(&plan, &items, &TaskStatePolicy::disabled());
        assert!(disabled.is_empty());

        let sparse = project_to_todo_items(&plan, &items, &TaskStatePolicy::sparse_plan());
        assert!(sparse.len() <= 8);

        let explicit = project_to_todo_items(&plan, &items, &TaskStatePolicy::explicit_todo());
        assert!(explicit.len() <= 10);

        let guided = project_to_todo_items(&plan, &items, &TaskStatePolicy::guided_current_task());
        assert!(guided.len() <= 4);

        // At most one in-progress so single-in-progress policies hold.
        let in_progress = explicit
            .iter()
            .filter(|item| item.status == TodoStatus::InProgress)
            .count();
        assert!(in_progress <= 1);
    }

    #[test]
    fn projection_never_includes_terminal_history() {
        let plan = plan_fixture();
        let items = vec![
            item_fixture("done", WorkItemStatus::Completed, 0, 0),
            item_fixture("next", WorkItemStatus::Pending, 1, 0),
        ];
        let projected = project_to_todo_items(&plan, &items, &TaskStatePolicy::explicit_todo());
        assert!(
            projected
                .iter()
                .all(|item| !item.id.starts_with("wi_done@")),
            "completed history must not erase into Todo context"
        );
    }

    #[test]
    fn stale_todo_mapping_cannot_mutate_newer_item() {
        let current = item_fixture("a", WorkItemStatus::Pending, 0, 2);
        let stale_todo = TodoItem {
            id: "wi_a@r1".to_string(),
            content: "work a".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::Medium,
            blocker: None,
        };
        let err = validate_todo_feedback(&stale_todo, &current, &WorkPlanEvidenceSnapshot::empty())
            .unwrap_err();
        assert!(matches!(err, TodoFeedbackError::Stale { .. }));
    }

    #[test]
    fn todo_completed_alone_never_satisfies_host_only() {
        let current = item_fixture("a", WorkItemStatus::InProgress, 0, 0);
        let todo = TodoItem {
            id: todo_id_for_work_item(&current),
            content: "work a".to_string(),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
            blocker: None,
        };
        let err = validate_todo_feedback(&todo, &current, &WorkPlanEvidenceSnapshot::empty())
            .unwrap_err();
        assert!(matches!(err, TodoFeedbackError::HostEvidenceRequired));
    }

    #[test]
    fn valid_in_progress_feedback_passes() {
        let current = item_fixture("a", WorkItemStatus::Pending, 0, 0);
        let todo = TodoItem {
            id: todo_id_for_work_item(&current),
            content: "work a".to_string(),
            status: TodoStatus::InProgress,
            priority: TodoPriority::Medium,
            blocker: None,
        };
        let target =
            validate_todo_feedback(&todo, &current, &WorkPlanEvidenceSnapshot::empty()).unwrap();
        assert_eq!(target, WorkItemStatus::InProgress);
    }
}
