//! Bounded WorkPlan checkpoint provenance (long-horizon M004).
//!
//! The continuation checkpoint remains a bounded handoff, not WorkPlan
//! storage. Checkpoints capture the current WorkPlan ID/revision plus a
//! bounded active-work projection; rollover revalidates the revision before
//! an epoch handoff becomes active. The full durable plan stays in
//! `codegg-core::work_plan` and remains reachable through WorkPlan
//! tools/recovery handles.
//!
//! Long-term references: ADR-0003 §12-§13 and
//! `plans/implementation/long-horizon-work-execution/004-*.md` §6.
//!
//! Ownership: `codegg-core::work_plan` owns these types. Serialization is
//! additive: checkpoints without `work_plan` (pre-M004) remain readable and
//! render safely; presence of `work_plan` never embeds the full plan.

use serde::{Deserialize, Serialize};

use super::model::{WorkItem, WorkItemStatus, WorkPlan, WorkPlanStatus};

// ── Bounds ────────────────────────────────────────────────────────────────

/// Maximum actionable/blocked item summaries carried in one checkpoint.
/// Deliberately smaller than the model projection page (max 8) so the
/// checkpoint stays a handoff, not a plan dump.
pub const MAX_CHECKPOINT_WORK_ITEMS: usize = 5;
/// Maximum description/next-action excerpt per summarized item.
pub const MAX_CHECKPOINT_ITEM_TEXT_CHARS: usize = 200;
/// Maximum current-phase excerpt.
pub const MAX_CHECKPOINT_PHASE_CHARS: usize = 256;
/// Maximum plan/status identifier chars (mirrors scope bounds).
pub const MAX_CHECKPOINT_ID_CHARS: usize = 256;

// ── Types ─────────────────────────────────────────────────────────────────

/// One bounded WorkItem summary inside a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlanCheckpointItem {
    pub id: String,
    pub revision: i64,
    pub status: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
}

/// Bounded WorkPlan provenance carried in a continuation checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlanCheckpointProvenance {
    pub plan_id: String,
    pub revision: i64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_item_id: Option<String>,
    #[serde(default)]
    pub actionable: Vec<WorkPlanCheckpointItem>,
    #[serde(default)]
    pub blocked: Vec<WorkPlanCheckpointItem>,
    pub source_digest: String,
    pub total_items: usize,
    pub actionable_count: usize,
    pub blocked_count: usize,
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn has_nul(value: &str) -> bool {
    value.contains('\0')
}

fn summarize_item(item: &WorkItem) -> WorkPlanCheckpointItem {
    WorkPlanCheckpointItem {
        id: item.id.as_str().to_string(),
        revision: item.revision,
        status: item.status.as_str().to_string(),
        description: bounded_text(item.description.trim(), MAX_CHECKPOINT_ITEM_TEXT_CHARS),
        next_action: item
            .next_action
            .as_deref()
            .map(|a| bounded_text(a.trim(), MAX_CHECKPOINT_ITEM_TEXT_CHARS))
            .filter(|a| !a.is_empty()),
    }
}

/// Build bounded checkpoint provenance from the durable plan.
///
/// Ordering is deterministic: the plan's `current_item_id` first (when still
/// non-terminal), then actionable items in `(position, id)` order, then
/// blocked remainder. Lists are truncated to [`MAX_CHECKPOINT_WORK_ITEMS`];
/// counts always describe the full plan so truncation never hides the
/// existence of remaining work.
pub fn build_provenance(plan: &WorkPlan, items: &[WorkItem]) -> WorkPlanCheckpointProvenance {
    let actionable = super::model::actionable_items(items);
    let actionable_ids: std::collections::HashSet<&str> =
        actionable.iter().map(|i| i.id.as_str()).collect();
    let mut ordered_actionable: Vec<&WorkItem> = Vec::new();
    if let Some(current_id) = plan.current_item_id.as_ref() {
        if let Some(current) = items.iter().find(|i| i.id == *current_id) {
            if !current.status.is_terminal()
                && matches!(
                    current.status,
                    WorkItemStatus::Pending
                        | WorkItemStatus::Actionable
                        | WorkItemStatus::InProgress
                )
            {
                ordered_actionable.push(current);
            }
        }
    }
    for item in &actionable {
        if !ordered_actionable.iter().any(|e| e.id == item.id) {
            ordered_actionable.push(item);
        }
    }
    let blocked: Vec<&WorkItem> = {
        let mut out: Vec<&WorkItem> = items
            .iter()
            .filter(|i| i.status == WorkItemStatus::Blocked)
            .collect();
        out.sort_by(|a, b| {
            a.position
                .cmp(&b.position)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });
        out
    };
    let actionable_summaries: Vec<WorkPlanCheckpointItem> = ordered_actionable
        .into_iter()
        .take(MAX_CHECKPOINT_WORK_ITEMS)
        .map(summarize_item)
        .collect();
    let blocked_summaries: Vec<WorkPlanCheckpointItem> = blocked
        .into_iter()
        .take(MAX_CHECKPOINT_WORK_ITEMS)
        .map(summarize_item)
        .collect();
    WorkPlanCheckpointProvenance {
        plan_id: plan.id.as_str().to_string(),
        revision: plan.revision,
        status: plan.status.as_str().to_string(),
        current_phase: plan
            .current_phase
            .as_deref()
            .map(|p| bounded_text(p.trim(), MAX_CHECKPOINT_PHASE_CHARS))
            .filter(|p| !p.is_empty()),
        current_item_id: plan
            .current_item_id
            .as_ref()
            .map(|id| id.as_str().to_string()),
        actionable: actionable_summaries,
        blocked: blocked_summaries,
        source_digest: plan.objective_digest.clone(),
        total_items: items.len(),
        actionable_count: actionable_ids.len(),
        blocked_count: items
            .iter()
            .filter(|i| i.status == WorkItemStatus::Blocked)
            .count(),
    }
}

/// Validate provenance shape without touching storage.
pub fn validate_provenance(value: &WorkPlanCheckpointProvenance) -> Result<(), String> {
    if value.plan_id.trim().is_empty() || value.plan_id.len() > MAX_CHECKPOINT_ID_CHARS {
        return Err("work plan checkpoint plan_id out of bounds".to_string());
    }
    if !value.plan_id.starts_with("wp_") {
        return Err("work plan checkpoint plan_id must carry wp_ prefix".to_string());
    }
    if has_nul(&value.plan_id) {
        return Err("work plan checkpoint plan_id must not contain NUL".to_string());
    }
    if value.revision < 0 {
        return Err("work plan checkpoint revision must be non-negative".to_string());
    }
    if WorkPlanStatus::parse(&value.status).is_none() {
        return Err(format!(
            "unknown work plan checkpoint status: {}",
            value.status
        ));
    }
    if let Some(phase) = value.current_phase.as_deref() {
        if phase.chars().count() > MAX_CHECKPOINT_PHASE_CHARS || has_nul(phase) {
            return Err("work plan checkpoint current_phase out of bounds".to_string());
        }
    }
    if let Some(item) = value.current_item_id.as_deref() {
        if item.len() > MAX_CHECKPOINT_ID_CHARS || has_nul(item) {
            return Err("work plan checkpoint current_item_id out of bounds".to_string());
        }
        if !item.starts_with("wi_") {
            return Err("work plan checkpoint current_item_id must carry wi_ prefix".to_string());
        }
    }
    if value.actionable.len() > MAX_CHECKPOINT_WORK_ITEMS {
        return Err("work plan checkpoint actionable list exceeds bound".to_string());
    }
    if value.blocked.len() > MAX_CHECKPOINT_WORK_ITEMS {
        return Err("work plan checkpoint blocked list exceeds bound".to_string());
    }
    for item in value.actionable.iter().chain(value.blocked.iter()) {
        if item.id.len() > MAX_CHECKPOINT_ID_CHARS
            || has_nul(&item.id)
            || !item.id.starts_with("wi_")
        {
            return Err("work plan checkpoint item id out of bounds".to_string());
        }
        if item.revision < 0 {
            return Err("work plan checkpoint item revision must be non-negative".to_string());
        }
        if item.description.trim().is_empty()
            || item.description.chars().count() > MAX_CHECKPOINT_ITEM_TEXT_CHARS
            || has_nul(&item.description)
        {
            return Err("work plan checkpoint item description out of bounds".to_string());
        }
        if let Some(action) = item.next_action.as_deref() {
            if action.chars().count() > MAX_CHECKPOINT_ITEM_TEXT_CHARS || has_nul(action) {
                return Err("work plan checkpoint next_action out of bounds".to_string());
            }
        }
    }
    if value.source_digest.trim().is_empty() || has_nul(&value.source_digest) {
        return Err("work plan checkpoint source_digest must not be empty".to_string());
    }
    Ok(())
}

/// Decode optional provenance from a checkpoint body.
///
/// Missing `work_plan` (pre-M004 checkpoints) yields `None` and remains a
/// valid legacy handoff. A present but malformed `work_plan` yields `Err`
/// so callers fail closed rather than installing a misleading handoff.
pub fn provenance_from_body(
    body: &serde_json::Value,
) -> Result<Option<WorkPlanCheckpointProvenance>, String> {
    let Some(raw) = body.get("work_plan") else {
        return Ok(None);
    };
    if raw.is_null() {
        return Ok(None);
    }
    let parsed: WorkPlanCheckpointProvenance = serde_json::from_value(raw.clone())
        .map_err(|e| format!("work plan checkpoint decode: {e}"))?;
    validate_provenance(&parsed)?;
    Ok(Some(parsed))
}

/// Serialize provenance into a checkpoint body map (additive key).
pub fn insert_provenance_into_body(
    body: &mut serde_json::Value,
    provenance: Option<&WorkPlanCheckpointProvenance>,
) {
    let Some(map) = body.as_object_mut() else {
        return;
    };
    match provenance {
        Some(value) => {
            if let Ok(json) = serde_json::to_value(value) {
                map.insert("work_plan".to_string(), json);
            }
        }
        None => {
            map.remove("work_plan");
        }
    }
}

/// Revalidate captured provenance against current durable state.
///
/// Returns `Ok(())` when the handoff may activate, `Err(reason)` when it
/// must abort/rebuild rather than install stale next action. `None`
/// captured (legacy checkpoint) never rejects on plan grounds; a captured
/// plan with no current durable plan is stale (explicit cancel/replacement
/// must not be papered over by an old handoff).
pub fn revalidate_against_current(
    captured: Option<&WorkPlanCheckpointProvenance>,
    current: Option<(&WorkPlan, &[WorkItem])>,
) -> Result<(), String> {
    let Some(want) = captured else {
        return Ok(());
    };
    let Some((plan, _)) = current else {
        return Err("active work plan missing for checkpoint provenance".to_string());
    };
    if plan.id.as_str() != want.plan_id {
        return Err(format!(
            "work plan identity changed (checkpoint {}, current {})",
            want.plan_id,
            plan.id.as_str()
        ));
    }
    if plan.revision != want.revision {
        return Err(format!(
            "stale work plan revision: checkpoint r{}, current r{}",
            want.revision, plan.revision
        ));
    }
    if plan.objective_digest != want.source_digest {
        return Err("work plan source digest changed".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_plan::model::{NewWorkItem, NewWorkPlan, WorkItemStatus, WorkPlanStatus};

    fn plan_fixture() -> (WorkPlan, Vec<WorkItem>) {
        use chrono::Utc;
        let now = Utc::now();
        let plan = WorkPlan {
            id: crate::work_plan::model::WorkPlanId("wp_1".to_string()),
            revision: 3,
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "implement epochs".to_string(),
            objective_digest: "sha256:abc".to_string(),
            origin_provenance: "turn:t".to_string(),
            status: WorkPlanStatus::Active,
            current_phase: Some("phase one".to_string()),
            current_item_id: None,
            created_at: now,
            updated_at: now,
            completed_at: None,
        };
        let mk = |id: &str, status: WorkItemStatus, pos: i64| WorkItem {
            id: crate::work_plan::model::WorkItemId(format!("wi_{id}")),
            plan_id: plan.id.clone(),
            revision: 0,
            position: pos,
            parent_item_id: None,
            dependencies: vec![],
            status,
            description: format!("work {id}"),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: if status == WorkItemStatus::Blocked {
                Some("waiting".to_string())
            } else {
                None
            },
            next_action: Some(format!("next {id}")),
            created_at: now,
            updated_at: now,
        };
        let items = vec![
            mk("a", WorkItemStatus::Pending, 0),
            mk("b", WorkItemStatus::Blocked, 1),
            mk("c", WorkItemStatus::Completed, 2),
        ];
        (plan, items)
    }

    #[test]
    fn provenance_is_bounded_and_deterministic() {
        let (plan, items) = plan_fixture();
        let first = build_provenance(&plan, &items);
        let second = build_provenance(&plan, &items);
        assert_eq!(first, second);
        assert!(first.actionable.len() <= MAX_CHECKPOINT_WORK_ITEMS);
        assert!(first.blocked.len() <= MAX_CHECKPOINT_WORK_ITEMS);
        assert_eq!(first.plan_id, "wp_1");
        assert_eq!(first.revision, 3);
        assert!(validate_provenance(&first).is_ok());
        let bytes = serde_json::to_string(&first).unwrap().len();
        assert!(bytes <= 4096, "provenance must stay small, got {bytes}");
    }

    #[test]
    fn missing_body_yields_legacy_none() {
        let body = serde_json::json!({"objective": "old"});
        assert!(provenance_from_body(&body).unwrap().is_none());
        let null_body = serde_json::json!({"work_plan": null});
        assert!(provenance_from_body(&null_body).unwrap().is_none());
    }

    #[test]
    fn round_trip_through_body() {
        let (plan, items) = plan_fixture();
        let provenance = build_provenance(&plan, &items);
        let mut body = serde_json::json!({"objective": "x"});
        insert_provenance_into_body(&mut body, Some(&provenance));
        let decoded = provenance_from_body(&body).unwrap().unwrap();
        assert_eq!(decoded, provenance);
    }

    #[test]
    fn revalidation_rejects_stale_revision() {
        let (mut plan, items) = plan_fixture();
        let provenance = build_provenance(&plan, &items);
        assert!(revalidate_against_current(Some(&provenance), Some((&plan, &items))).is_ok());
        plan.revision = 4;
        let err = revalidate_against_current(Some(&provenance), Some((&plan, &items))).unwrap_err();
        assert!(err.contains("stale"), "unexpected: {err}");
        let missing = revalidate_against_current(Some(&provenance), None).unwrap_err();
        assert!(missing.contains("missing"));
        // Legacy checkpoint without provenance never rejects on plan grounds.
        assert!(revalidate_against_current(None, Some((&plan, &items))).is_ok());
        assert!(revalidate_against_current(None, None).is_ok());
    }

    #[test]
    fn large_plan_stays_bounded() {
        let (plan, _) = plan_fixture();
        let now = chrono::Utc::now();
        let items: Vec<WorkItem> = (0..20)
            .map(|i| WorkItem {
                id: crate::work_plan::model::WorkItemId(format!("wi_item{i:02}")),
                plan_id: plan.id.clone(),
                revision: 0,
                position: i,
                parent_item_id: None,
                dependencies: vec![],
                status: WorkItemStatus::Pending,
                description: format!("work item {i} with some description"),
                acceptance: vec![],
                evidence: vec![],
                owner_run_id: None,
                owner_job_id: None,
                attempts: 0,
                blocker: None,
                next_action: Some("do it".to_string()),
                created_at: now,
                updated_at: now,
            })
            .collect();
        let provenance = build_provenance(&plan, &items);
        assert!(provenance.actionable.len() <= MAX_CHECKPOINT_WORK_ITEMS);
        assert_eq!(provenance.total_items, 20);
        assert_eq!(provenance.actionable_count, 20);
    }

    #[test]
    fn unused_new_types_stay_importable() {
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
