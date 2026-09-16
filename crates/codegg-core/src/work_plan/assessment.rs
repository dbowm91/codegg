//! Pure WorkPlan completion assessment (long-horizon M003).
//!
//! The assessment is read-only and deterministic: it derives one of five
//! states from the durable plan/items plus a bounded host-evidence snapshot.
//! Model prose never enters the decision; only structured item state and
//! canonical evidence do.

use serde::{Deserialize, Serialize};

use super::evidence::{
    item_has_failed_evidence, item_has_in_progress_evidence, item_is_satisfied,
    item_is_user_judgment_only, WorkPlanEvidenceSnapshot,
};
use super::model::{actionable_items, WorkItem, WorkItemId, WorkPlan, WorkPlanStatus};

/// Bounded preview lengths for arbiter diagnostics. Diagnostics carry
/// identifiers and short excerpts only, never full plan content.
pub const MAX_ASSESSMENT_TEXT_CHARS: usize = 200;
pub const MAX_ASSESSMENT_REASON_CHARS: usize = 500;

/// Host-owned completion assessment for one WorkPlan.
///
/// `Blocked` here is a WorkPlan assessment/item state, not a new GoalStatus.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPlanCompletionAssessment {
    Complete {
        reason: String,
    },
    ActionableWorkRemaining {
        current_item_id: WorkItemId,
        description: String,
        unmet_count: usize,
        next_action: Option<String>,
    },
    Blocked {
        item_id: Option<WorkItemId>,
        blocker: String,
    },
    AwaitingUserJudgment {
        reasons: Vec<String>,
    },
    InFlight {
        item_id: Option<WorkItemId>,
        handle_kind: String,
        handle_id: String,
    },
}

impl WorkPlanCompletionAssessment {
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::Complete { .. } => "complete",
            Self::ActionableWorkRemaining { .. } => "actionable_work_remaining",
            Self::Blocked { .. } => "blocked",
            Self::AwaitingUserJudgment { .. } => "awaiting_user_judgment",
            Self::InFlight { .. } => "in_flight",
        }
    }

    pub fn allows_completion(&self) -> bool {
        matches!(
            self,
            Self::Complete { .. } | Self::AwaitingUserJudgment { .. }
        )
    }

    pub fn requires_continuation(&self) -> bool {
        matches!(self, Self::ActionableWorkRemaining { .. })
    }
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

fn item_description_preview(item: &WorkItem) -> String {
    bounded_text(item.description.trim(), MAX_ASSESSMENT_TEXT_CHARS)
}

fn item_next_action_preview(item: &WorkItem) -> Option<String> {
    item.next_action
        .as_deref()
        .map(|action| bounded_text(action.trim(), MAX_ASSESSMENT_TEXT_CHARS))
        .filter(|action| !action.is_empty())
}

fn unmet_acceptance_count(item: &WorkItem) -> usize {
    item.acceptance
        .iter()
        .filter(|c| c.disposition == super::model::WorkAcceptanceDisposition::Unmet)
        .count()
}

/// Pure assessment of plan completion from structured state and canonical
/// evidence. Never reads model prose, transcript summaries, or Todo text.
pub fn assess_work_plan(
    plan: &WorkPlan,
    items: &[WorkItem],
    evidence: &WorkPlanEvidenceSnapshot,
) -> WorkPlanCompletionAssessment {
    if matches!(
        plan.status,
        WorkPlanStatus::Completed | WorkPlanStatus::Cancelled
    ) {
        return WorkPlanCompletionAssessment::Complete {
            reason: bounded_text(
                &format!("plan {} is {}", plan.id.as_str(), plan.status.as_str()),
                MAX_ASSESSMENT_REASON_CHARS,
            ),
        };
    }

    if plan.status == WorkPlanStatus::Blocked {
        if let Some(blocked) = items
            .iter()
            .find(|item| item.status == super::model::WorkItemStatus::Blocked)
        {
            return WorkPlanCompletionAssessment::Blocked {
                item_id: Some(blocked.id.clone()),
                blocker: bounded_text(
                    blocked.blocker.as_deref().unwrap_or("plan is blocked"),
                    MAX_ASSESSMENT_REASON_CHARS,
                ),
            };
        }
        return WorkPlanCompletionAssessment::Blocked {
            item_id: None,
            blocker: bounded_text("plan is blocked", MAX_ASSESSMENT_REASON_CHARS),
        };
    }

    let required: Vec<&WorkItem> = items
        .iter()
        .filter(|item| !item.status.is_terminal())
        .collect();

    if required.is_empty() {
        // No remaining items, but a Completed item without host satisfaction
        // must not read as done: claimed completion without canonical
        // evidence stays actionable.
        if let Some(unsatisfied) = items.iter().find(|item| {
            item.status == super::model::WorkItemStatus::Completed
                && !item_is_satisfied(item, evidence)
        }) {
            return WorkPlanCompletionAssessment::ActionableWorkRemaining {
                current_item_id: unsatisfied.id.clone(),
                description: item_description_preview(unsatisfied),
                unmet_count: unmet_acceptance_count(unsatisfied),
                next_action: item_next_action_preview(unsatisfied).or_else(|| {
                    Some("establish host-owned evidence before completing".to_string())
                }),
            };
        }
        return WorkPlanCompletionAssessment::Complete {
            reason: bounded_text(
                "all required items are complete",
                MAX_ASSESSMENT_REASON_CHARS,
            ),
        };
    }

    let actionable = actionable_items(items);
    if !actionable.is_empty() {
        let deterministic: Vec<&&WorkItem> = actionable
            .iter()
            .filter(|item| !item_is_user_judgment_only(item, evidence))
            .collect();
        if deterministic.is_empty() {
            let mut reasons: Vec<String> = actionable
                .iter()
                .take(4)
                .map(|item| {
                    bounded_text(
                        &format!("{}: awaiting user judgment", item_description_preview(item)),
                        MAX_ASSESSMENT_TEXT_CHARS,
                    )
                })
                .collect();
            if reasons.is_empty() {
                reasons.push("remaining criteria require user judgment".to_string());
            }
            return WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons };
        }
        let current = deterministic[0];
        return WorkPlanCompletionAssessment::ActionableWorkRemaining {
            current_item_id: current.id.clone(),
            description: item_description_preview(current),
            unmet_count: unmet_acceptance_count(current),
            next_action: item_next_action_preview(current),
        };
    }

    // No actionable items: examine in-progress, blocked, and dependency-gated
    // remainder in a fixed precedence so hosts and tests observe one order.
    let in_progress: Vec<&&WorkItem> = required
        .iter()
        .filter(|item| item.status == super::model::WorkItemStatus::InProgress)
        .collect();
    if !in_progress.is_empty() {
        // Failed canonical evidence means retryable work can proceed.
        if let Some(failed) = in_progress
            .iter()
            .find(|item| item_has_failed_evidence(item, evidence))
        {
            return WorkPlanCompletionAssessment::ActionableWorkRemaining {
                current_item_id: failed.id.clone(),
                description: item_description_preview(failed),
                unmet_count: unmet_acceptance_count(failed),
                next_action: item_next_action_preview(failed).or_else(|| {
                    Some("resolve the failed host-owned evidence, then continue".to_string())
                }),
            };
        }
        if let Some(live) = in_progress
            .iter()
            .find(|item| item_has_in_progress_evidence(item, evidence))
        {
            let (kind, id) = live_handle_for(live, evidence);
            return WorkPlanCompletionAssessment::InFlight {
                item_id: Some(live.id.clone()),
                handle_kind: kind,
                handle_id: id,
            };
        }
        // Owner provenance without a resolved live handle still indicates
        // delegated work that may be waitable, but without a canonical
        // InProgress ref it is not a verified wait: surface it as InFlight
        // with the owner handle so the caller can reload terminal evidence.
        if let Some(owned) = in_progress
            .iter()
            .find(|item| item.owner_run_id.is_some() || item.owner_job_id.is_some())
        {
            let (kind, id) = owner_handle_for(owned);
            return WorkPlanCompletionAssessment::InFlight {
                item_id: Some(owned.id.clone()),
                handle_kind: kind,
                handle_id: id,
            };
        }
        // InProgress with no live handle and no failure still needs progress.
        let current = in_progress[0];
        return WorkPlanCompletionAssessment::ActionableWorkRemaining {
            current_item_id: current.id.clone(),
            description: item_description_preview(current),
            unmet_count: unmet_acceptance_count(current),
            next_action: item_next_action_preview(current),
        };
    }

    if let Some(blocked) = required
        .iter()
        .find(|item| item.status == super::model::WorkItemStatus::Blocked)
    {
        return WorkPlanCompletionAssessment::Blocked {
            item_id: Some(blocked.id.clone()),
            blocker: bounded_text(
                blocked.blocker.as_deref().unwrap_or("item is blocked"),
                MAX_ASSESSMENT_REASON_CHARS,
            ),
        };
    }

    // Remaining Pending/Actionable items are gated by unmet dependencies.
    if let Some(waiting) = required.iter().find(|item| {
        matches!(
            item.status,
            super::model::WorkItemStatus::Pending | super::model::WorkItemStatus::Actionable
        )
    }) {
        let completed: std::collections::BTreeSet<&str> = items
            .iter()
            .filter(|item| item.status == super::model::WorkItemStatus::Completed)
            .map(|item| item.id.as_str())
            .collect();
        if let Some(missing) = waiting
            .dependencies
            .iter()
            .find(|dep| !completed.contains(dep.as_str()))
        {
            return WorkPlanCompletionAssessment::Blocked {
                item_id: Some(waiting.id.clone()),
                blocker: bounded_text(
                    &format!(
                        "waiting on dependency {} for {}",
                        missing.as_str(),
                        item_description_preview(waiting)
                    ),
                    MAX_ASSESSMENT_REASON_CHARS,
                ),
            };
        }
    }

    if required
        .iter()
        .all(|item| item_is_user_judgment_only(item, evidence))
    {
        let reasons: Vec<String> = required
            .iter()
            .take(4)
            .map(|item| {
                bounded_text(
                    &format!("{}: awaiting user judgment", item_description_preview(item)),
                    MAX_ASSESSMENT_TEXT_CHARS,
                )
            })
            .collect();
        return WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons };
    }

    WorkPlanCompletionAssessment::Blocked {
        item_id: required.first().map(|item| item.id.clone()),
        blocker: bounded_text(
            "required work cannot proceed without resolving the current blocker",
            MAX_ASSESSMENT_REASON_CHARS,
        ),
    }
}

fn live_handle_for(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> (String, String) {
    use super::evidence::HostEvidenceStatus;
    for evidence in &item.evidence {
        if snapshot.lookup(evidence.kind, evidence.ref_id.as_str())
            == HostEvidenceStatus::InProgress
        {
            return (
                bounded_text(evidence.kind.as_str(), 64),
                bounded_text(evidence.ref_id.as_str(), MAX_ASSESSMENT_TEXT_CHARS),
            );
        }
    }
    owner_handle_for(item)
}

fn owner_handle_for(item: &WorkItem) -> (String, String) {
    if let Some(run) = item.owner_run_id.as_deref() {
        return (
            "delegated_run".to_string(),
            bounded_text(run, MAX_ASSESSMENT_TEXT_CHARS),
        );
    }
    if let Some(job) = item.owner_job_id.as_deref() {
        return (
            "scheduler_job".to_string(),
            bounded_text(job, MAX_ASSESSMENT_TEXT_CHARS),
        );
    }
    ("unknown".to_string(), "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_plan::evidence::HostEvidenceStatus;
    use crate::work_plan::model::{
        WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind, WorkEvidenceRef, WorkItemId,
        WorkItemStatus, WorkPlanId, WorkPlanStatus,
    };
    use chrono::Utc;

    fn plan_fixture(status: WorkPlanStatus) -> WorkPlan {
        let now = Utc::now();
        WorkPlan {
            id: WorkPlanId("wp_1".to_string()),
            revision: 0,
            session_id: "s".to_string(),
            project_id: "p".to_string(),
            origin_turn_id: None,
            goal_id: None,
            objective: "objective".to_string(),
            objective_digest: "sha256:x".to_string(),
            origin_provenance: "turn:t".to_string(),
            status,
            current_phase: None,
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
            plan_id: WorkPlanId("wp_1".to_string()),
            revision: 0,
            position,
            parent_item_id: None,
            dependencies: vec![],
            status,
            description: format!("work {id}"),
            acceptance: vec![WorkAcceptance {
                description: "criterion".to_string(),
                disposition: WorkAcceptanceDisposition::Unmet,
                note: None,
            }],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: Some("do the next step".to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn complete_when_all_required_done_and_satisfied() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut done = item_fixture("a", WorkItemStatus::Completed, 0);
        done.acceptance = vec![WorkAcceptance {
            description: "c".to_string(),
            disposition: WorkAcceptanceDisposition::Satisfied,
            note: None,
        }];
        let assessment = assess_work_plan(&plan, &[done], &WorkPlanEvidenceSnapshot::empty());
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::Complete { .. }
        ));
    }

    #[test]
    fn completed_without_host_signal_stays_actionable() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut done = item_fixture("a", WorkItemStatus::Completed, 0);
        done.acceptance = vec![];
        done.evidence = vec![];
        let assessment = assess_work_plan(&plan, &[done], &WorkPlanEvidenceSnapshot::empty());
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
        ));
    }

    #[test]
    fn actionable_work_remaining_for_pending_with_unmet() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let items = [
            item_fixture("a", WorkItemStatus::Completed, 0),
            item_fixture("b", WorkItemStatus::Pending, 1),
        ];
        // Mark the completed item satisfied so only the pending item drives
        // the assessment.
        let mut satisfied = items[0].clone();
        satisfied.acceptance = vec![WorkAcceptance {
            description: "c".to_string(),
            disposition: WorkAcceptanceDisposition::Satisfied,
            note: None,
        }];
        let assessment = assess_work_plan(
            &plan,
            &[satisfied, items[1].clone()],
            &WorkPlanEvidenceSnapshot::empty(),
        );
        match assessment {
            WorkPlanCompletionAssessment::ActionableWorkRemaining {
                current_item_id, ..
            } => assert_eq!(current_item_id.as_str(), "wi_b"),
            other => panic!("expected actionable, got {other:?}"),
        }
    }

    #[test]
    fn failed_evidence_keeps_item_actionable() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut item = item_fixture("a", WorkItemStatus::InProgress, 0);
        item.evidence = vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-1".to_string(),
            detail: None,
        }];
        let snapshot = WorkPlanEvidenceSnapshot::empty().with_entry(
            WorkEvidenceKind::TestJob,
            "job-1",
            HostEvidenceStatus::Failed,
        );
        let assessment = assess_work_plan(&plan, &[item], &snapshot);
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
        ));
    }

    #[test]
    fn in_flight_for_live_handle_without_actionable() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut item = item_fixture("a", WorkItemStatus::InProgress, 0);
        item.acceptance = vec![];
        item.evidence = vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-1".to_string(),
            detail: None,
        }];
        let snapshot = WorkPlanEvidenceSnapshot::empty().with_entry(
            WorkEvidenceKind::TestJob,
            "job-1",
            HostEvidenceStatus::InProgress,
        );
        let assessment = assess_work_plan(&plan, &[item], &snapshot);
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::InFlight { .. }
        ));
    }

    #[test]
    fn blocked_when_only_blocked_remains() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut item = item_fixture("a", WorkItemStatus::Blocked, 0);
        item.blocker = Some("waiting on review".to_string());
        item.acceptance = vec![];
        let assessment = assess_work_plan(&plan, &[item], &WorkPlanEvidenceSnapshot::empty());
        match assessment {
            WorkPlanCompletionAssessment::Blocked { blocker, .. } => {
                assert!(blocker.contains("waiting on review"));
            }
            other => panic!("expected blocked, got {other:?}"),
        }
    }

    #[test]
    fn awaiting_user_judgment_when_only_judgment_remains() {
        let plan = plan_fixture(WorkPlanStatus::Active);
        let mut item = item_fixture("a", WorkItemStatus::Pending, 0);
        item.acceptance = vec![WorkAcceptance {
            description: "owner signs off".to_string(),
            disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
            note: None,
        }];
        let assessment = assess_work_plan(&plan, &[item], &WorkPlanEvidenceSnapshot::empty());
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::AwaitingUserJudgment { .. }
        ));
    }

    #[test]
    fn terminal_plan_is_complete_without_arbitration() {
        let plan = plan_fixture(WorkPlanStatus::Completed);
        let assessment = assess_work_plan(&plan, &[], &WorkPlanEvidenceSnapshot::empty());
        assert!(matches!(
            assessment,
            WorkPlanCompletionAssessment::Complete { .. }
        ));
    }
}
