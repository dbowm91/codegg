//! Bounded WorkPlan projection for model context (long-horizon M003).
//!
//! The projection defaults to the current/actionable slice and supports
//! bounded pagination/lookup for larger plans. Completed history is never
//! dumped on every turn: callers opt into it explicitly and still receive a
//! bounded page.

use serde::{Deserialize, Serialize};

use super::model::{WorkItem, WorkItemId, WorkItemStatus, WorkPlan};

/// Maximum items in one projection page. Deliberately below the durable
/// 64-item plan cap and within TodoState's 10-12 projection budget so the
/// model context stays bounded even for large plans.
pub const MAX_PROJECTION_ITEMS: usize = 8;
/// Maximum serialized projection bytes. Callers truncate pages until the
/// JSON fits.
pub const MAX_PROJECTION_BYTES: usize = 4096;
/// Maximum description/next-action/blocker excerpt per item.
pub const MAX_PROJECTION_TEXT_CHARS: usize = 200;
/// Maximum objective preview per plan.
pub const MAX_PROJECTION_OBJECTIVE_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkPlanProjectionParams {
    pub offset: usize,
    pub limit: usize,
    pub include_completed: bool,
}

impl Default for WorkPlanProjectionParams {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: 5,
            include_completed: false,
        }
    }
}

impl WorkPlanProjectionParams {
    pub fn bounded(limit: usize) -> Self {
        Self {
            offset: 0,
            limit: limit.clamp(1, MAX_PROJECTION_ITEMS),
            include_completed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlanItemSummary {
    pub id: WorkItemId,
    pub revision: i64,
    pub status: WorkItemStatus,
    pub description: String,
    pub position: i64,
    pub unmet_count: usize,
    pub satisfied_count: usize,
    pub judgment_count: usize,
    pub evidence_count: usize,
    pub blocker: Option<String>,
    pub next_action: Option<String>,
    pub has_owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlanProjection {
    pub plan_id: super::model::WorkPlanId,
    pub revision: i64,
    pub status: super::model::WorkPlanStatus,
    pub objective_preview: String,
    pub current_phase: Option<String>,
    pub current_item_id: Option<WorkItemId>,
    pub total_items: usize,
    pub completed_items: usize,
    pub actionable_count: usize,
    pub blocked_count: usize,
    pub in_progress_count: usize,
    pub items: Vec<WorkPlanItemSummary>,
    pub truncated: bool,
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

pub fn summarize_item(item: &WorkItem) -> WorkPlanItemSummary {
    use super::model::WorkAcceptanceDisposition as Disposition;
    WorkPlanItemSummary {
        id: item.id.clone(),
        revision: item.revision,
        status: item.status,
        description: bounded_text(item.description.trim(), MAX_PROJECTION_TEXT_CHARS),
        position: item.position,
        unmet_count: item
            .acceptance
            .iter()
            .filter(|c| c.disposition == Disposition::Unmet)
            .count(),
        satisfied_count: item
            .acceptance
            .iter()
            .filter(|c| c.disposition == Disposition::Satisfied)
            .count(),
        judgment_count: item
            .acceptance
            .iter()
            .filter(|c| c.disposition == Disposition::RequiresUserJudgment)
            .count(),
        evidence_count: item.evidence.len(),
        blocker: item
            .blocker
            .as_deref()
            .map(|blocker| bounded_text(blocker.trim(), MAX_PROJECTION_TEXT_CHARS))
            .filter(|blocker| !blocker.is_empty()),
        next_action: item
            .next_action
            .as_deref()
            .map(|action| bounded_text(action.trim(), MAX_PROJECTION_TEXT_CHARS))
            .filter(|action| !action.is_empty()),
        has_owner: item.owner_run_id.is_some() || item.owner_job_id.is_some(),
    }
}

/// Project the current/actionable slice by default.
///
/// Ordering is stable: the plan's `current_item_id` first (when it is still
/// non-terminal), then actionable items in `(position, id)` order, then
/// in-progress/blocked remainder. Completed/cancelled items are excluded
/// unless `include_completed` is set, and pagination applies after ordering.
pub fn project_work_plan(
    plan: &WorkPlan,
    items: &[WorkItem],
    params: &WorkPlanProjectionParams,
) -> WorkPlanProjection {
    let limit = params.limit.clamp(1, MAX_PROJECTION_ITEMS);
    let actionable = super::model::actionable_items(items);
    let actionable_ids: std::collections::HashSet<&str> =
        actionable.iter().map(|item| item.id.as_str()).collect();

    let mut ordered: Vec<&WorkItem> = Vec::new();
    if let Some(current_id) = plan.current_item_id.as_ref() {
        if let Some(current) = items.iter().find(|item| item.id == *current_id) {
            if !current.status.is_terminal() {
                ordered.push(current);
            }
        }
    }
    for item in &actionable {
        if !ordered.iter().any(|existing| existing.id == item.id) {
            ordered.push(item);
        }
    }
    // Include in-progress/blocked remainder so the model sees why no
    // actionable work exists, still bounded by the page limit.
    if ordered.len() < limit {
        let mut seen: std::collections::HashSet<&str> =
            ordered.iter().map(|item| item.id.as_str()).collect();
        for item in items.iter().filter(|item| !item.status.is_terminal()) {
            if seen.insert(item.id.as_str()) {
                ordered.push(item);
                if ordered.len() >= limit {
                    break;
                }
            }
        }
    }
    if params.include_completed && ordered.len() < limit {
        let mut seen: std::collections::HashSet<&str> =
            ordered.iter().map(|item| item.id.as_str()).collect();
        for item in items.iter().filter(|item| item.status.is_terminal()) {
            if seen.insert(item.id.as_str()) {
                ordered.push(item);
                if ordered.len() >= limit {
                    break;
                }
            }
        }
    }
    // Filter completed unless explicitly requested (the default path above
    // only adds them when requested).
    let mut ordered: Vec<&WorkItem> = if params.include_completed {
        ordered
    } else {
        ordered
            .into_iter()
            .filter(|item| !item.status.is_terminal())
            .collect()
    };
    // Stable order is already established; pagination slices it.
    let total_ordered = ordered.len();
    let start = params.offset.min(total_ordered);
    let end = (start + limit).min(total_ordered);
    ordered = ordered[start..end].to_vec();
    let mut summaries: Vec<WorkPlanItemSummary> =
        ordered.iter().map(|item| summarize_item(item)).collect();

    // Byte-cap enforcement: drop trailing items until the JSON fits. The
    // counts below always describe the full plan, so dropping never hides
    // the existence of remaining work.
    let mut truncated = total_ordered > end;
    while summaries.len() > 1 {
        let candidate = WorkPlanProjection {
            plan_id: plan.id.clone(),
            revision: plan.revision,
            status: plan.status,
            objective_preview: bounded_text(plan.objective.trim(), MAX_PROJECTION_OBJECTIVE_CHARS),
            current_phase: plan.current_phase.clone(),
            current_item_id: plan.current_item_id.clone(),
            total_items: items.len(),
            completed_items: items
                .iter()
                .filter(|item| item.status == WorkItemStatus::Completed)
                .count(),
            actionable_count: actionable_ids.len(),
            blocked_count: items
                .iter()
                .filter(|item| item.status == WorkItemStatus::Blocked)
                .count(),
            in_progress_count: items
                .iter()
                .filter(|item| item.status == WorkItemStatus::InProgress)
                .count(),
            items: summaries.clone(),
            truncated,
        };
        let bytes = serde_json::to_string(&candidate)
            .map(|json| json.len())
            .unwrap_or(0);
        if bytes <= MAX_PROJECTION_BYTES {
            break;
        }
        summaries.pop();
        truncated = true;
    }

    WorkPlanProjection {
        plan_id: plan.id.clone(),
        revision: plan.revision,
        status: plan.status,
        objective_preview: bounded_text(plan.objective.trim(), MAX_PROJECTION_OBJECTIVE_CHARS),
        current_phase: plan.current_phase.clone(),
        current_item_id: plan.current_item_id.clone(),
        total_items: items.len(),
        completed_items: items
            .iter()
            .filter(|item| item.status == WorkItemStatus::Completed)
            .count(),
        actionable_count: actionable_ids.len(),
        blocked_count: items
            .iter()
            .filter(|item| item.status == WorkItemStatus::Blocked)
            .count(),
        in_progress_count: items
            .iter()
            .filter(|item| item.status == WorkItemStatus::InProgress)
            .count(),
        items: summaries,
        truncated,
    }
}

/// Bounded single-item lookup for larger plans.
pub fn lookup_item_summary(items: &[WorkItem], id: &WorkItemId) -> Option<WorkPlanItemSummary> {
    items.iter().find(|item| item.id == *id).map(summarize_item)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_plan::model::{
        NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkItemStatus,
        WorkPlanStatus,
    };
    use chrono::Utc;

    fn plan_fixture() -> WorkPlan {
        let now = Utc::now();
        WorkPlan {
            id: crate::work_plan::model::WorkPlanId("wp_1".to_string()),
            revision: 3,
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "implement the long-horizon projection".to_string(),
            objective_digest: "sha256:x".to_string(),
            origin_provenance: "turn:t".to_string(),
            status: WorkPlanStatus::Active,
            current_phase: Some("projection".to_string()),
            current_item_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        }
    }

    fn item_fixture(id: &str, status: WorkItemStatus, position: i64) -> WorkItem {
        let now = Utc::now();
        WorkItem {
            id: WorkItemId(format!("wi_{id}")),
            plan_id: crate::work_plan::model::WorkPlanId("wp_1".to_string()),
            revision: 0,
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
    fn default_projection_excludes_completed_history() {
        let plan = plan_fixture();
        let mut items = Vec::new();
        for index in 0..12 {
            items.push(item_fixture(
                &format!("done{index}"),
                WorkItemStatus::Completed,
                index,
            ));
        }
        items.push(item_fixture("next", WorkItemStatus::Pending, 12));
        let projection = project_work_plan(&plan, &items, &WorkPlanProjectionParams::default());
        assert_eq!(projection.total_items, 13);
        assert_eq!(projection.completed_items, 12);
        assert!(
            projection
                .items
                .iter()
                .all(|item| item.status != WorkItemStatus::Completed),
            "default projection must not dump completed history"
        );
        assert!(projection.items.len() <= MAX_PROJECTION_ITEMS);
        let bytes = serde_json::to_string(&projection).unwrap().len();
        assert!(bytes <= MAX_PROJECTION_BYTES);
    }

    #[test]
    fn large_plan_stays_bounded_with_pagination() {
        let plan = plan_fixture();
        let items: Vec<WorkItem> = (0..20)
            .map(|index| item_fixture(&format!("item{index}"), WorkItemStatus::Pending, index))
            .collect();
        let first = project_work_plan(
            &plan,
            &items,
            &WorkPlanProjectionParams {
                offset: 0,
                limit: 5,
                include_completed: false,
            },
        );
        let second = project_work_plan(
            &plan,
            &items,
            &WorkPlanProjectionParams {
                offset: 5,
                limit: 5,
                include_completed: false,
            },
        );
        assert_eq!(first.items.len(), 5);
        assert_eq!(second.items.len(), 5);
        assert_ne!(first.items[0].id, second.items[0].id);
        assert!(first.truncated || second.truncated || first.total_items > 5);
    }

    #[test]
    fn lookup_returns_bounded_summary() {
        let items = vec![item_fixture("a", WorkItemStatus::Blocked, 0)];
        let summary = lookup_item_summary(&items, &WorkItemId("wi_a".to_string())).unwrap();
        assert_eq!(summary.id.as_str(), "wi_a");
        let missing = lookup_item_summary(&items, &WorkItemId("wi_missing".to_string()));
        assert!(missing.is_none());
    }

    #[test]
    fn unused_new_types_stay_importable_for_docs() {
        let _ = NewWorkPlan {
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "o".to_string(),
            origin_provenance: "turn:t".to_string(),
            current_phase: None,
        };
        let _ = NewWorkItem {
            parent_item_id: None,
            dependencies: vec![],
            status: WorkItemStatus::Pending,
            description: "d".to_string(),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            blocker: None,
            next_action: None,
        };
    }
}
