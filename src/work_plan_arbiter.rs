//! Host-owned WorkPlan completion arbiter (long-horizon M003).
//!
//! The arbiter runs at the terminal-answer boundary before an ordinary turn
//! or a Goal-bound request is considered done. When required actionable work
//! remains it produces one bounded control message describing the current
//! item, the unmet condition, and the next action. Repeated failure goes
//! through the existing turn/tool/time/token limits owned by the agent loop,
//! never through unbounded recursion here.
//!
//! GoalVerification remains the final authority for Goal completion; the
//! WorkPlan assessment is an additional prerequisite, not a replacement.

use codegg_core::work_plan::{
    assess_work_plan, WorkItem, WorkPlan, WorkPlanCompletionAssessment, WorkPlanEvidenceSnapshot,
    WorkPlanStore,
};
use sqlx::SqlitePool;

pub const MAX_ARBITER_MESSAGE_CHARS: usize = 1000;

/// Arbiter decision at a completion boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArbiterDecision {
    /// No active plan, or the plan allows completion. The caller proceeds to
    /// the existing verifier/turn-close path.
    AllowCompletion,
    /// Required actionable work remains: continue with this bounded prompt.
    ContinueWithPrompt(String),
    /// Canonical work is still running: wait/poll the named handle.
    WaitForHandle {
        handle_kind: String,
        handle_id: String,
    },
    /// Required work is blocked on a named blocker.
    BlockedReport(String),
    /// Only user-judgment criteria remain: return control to the user.
    NeedsUserJudgment(String),
    /// Plan/evidence could not be read: never mark complete.
    Inconclusive(String),
}

impl ArbiterDecision {
    pub fn should_continue(&self) -> bool {
        matches!(
            self,
            Self::ContinueWithPrompt(_) | Self::WaitForHandle { .. }
        )
    }
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

/// Build the bounded "required work remains" control message.
///
/// Describes the current item, the unmet condition, and the next action. The
/// message never dumps full plan history or evidence payloads.
pub fn build_required_work_message(
    current_item_id: &codegg_core::work_plan::WorkItemId,
    description: &str,
    unmet_count: usize,
    next_action: Option<&str>,
) -> String {
    let mut message = format!(
        "WorkPlan arbiter: required work remains (item {}: {}",
        current_item_id.as_str(),
        bounded_text(description.trim(), 200)
    );
    if unmet_count > 0 {
        message.push_str(&format!("; {unmet_count} unmet criterion/criteria"));
    }
    message.push(')');
    if let Some(action) = next_action
        .map(str::trim)
        .filter(|action| !action.is_empty())
    {
        message.push_str(&format!(". Next action: {}", bounded_text(action, 200)));
    } else {
        message.push_str(". Continue with the current item using an allowed WorkPlan update.");
    }
    message.push_str(" Use work_plan_get for the bounded current slice; do not dump full history.");
    bounded_text(&message, MAX_ARBITER_MESSAGE_CHARS)
}

/// Assess the active plan for a session, if any.
///
/// Returns `None` when no active plan exists (legacy behavior preserved) or
/// `Some((plan, items, assessment))` otherwise. Storage/evidence failures
/// yield an `Inconclusive` assessment decision via [`decide_from_assessment`],
/// never silent completion.
pub async fn assess_active_plan(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<(WorkPlan, Vec<WorkItem>, WorkPlanCompletionAssessment)>, String> {
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .active_for_session(session_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(plan) = plan else {
        return Ok(None);
    };
    let items = store
        .list_items(&plan.id)
        .await
        .map_err(|error| error.to_string())?;
    let evidence = crate::work_plan_evidence::assemble(pool, &items)
        .await
        .unwrap_or_else(|_| WorkPlanEvidenceSnapshot::empty());
    Ok(Some((
        plan.clone(),
        items.clone(),
        assess_work_plan(&plan, &items, &evidence),
    )))
}

/// Assess the plan bound to a Goal, if any.
pub async fn assess_goal_plan(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<Option<(WorkPlan, Vec<WorkItem>, WorkPlanCompletionAssessment)>, String> {
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .active_for_goal(goal_id)
        .await
        .map_err(|error| error.to_string())?;
    let Some(plan) = plan else {
        return Ok(None);
    };
    let items = store
        .list_items(&plan.id)
        .await
        .map_err(|error| error.to_string())?;
    let evidence = crate::work_plan_evidence::assemble(pool, &items)
        .await
        .unwrap_or_else(|_| WorkPlanEvidenceSnapshot::empty());
    Ok(Some((
        plan.clone(),
        items.clone(),
        assess_work_plan(&plan, &items, &evidence),
    )))
}

pub fn decide_from_assessment(assessment: &WorkPlanCompletionAssessment) -> ArbiterDecision {
    match assessment {
        WorkPlanCompletionAssessment::Complete { .. } => ArbiterDecision::AllowCompletion,
        WorkPlanCompletionAssessment::ActionableWorkRemaining {
            current_item_id,
            description,
            unmet_count,
            next_action,
        } => ArbiterDecision::ContinueWithPrompt(build_required_work_message(
            current_item_id,
            description,
            *unmet_count,
            next_action.as_deref(),
        )),
        WorkPlanCompletionAssessment::InFlight {
            handle_kind,
            handle_id,
            ..
        } => ArbiterDecision::WaitForHandle {
            handle_kind: bounded_text(handle_kind, 64),
            handle_id: bounded_text(handle_id, 200),
        },
        WorkPlanCompletionAssessment::Blocked { blocker, .. } => {
            ArbiterDecision::BlockedReport(bounded_text(blocker, 500))
        }
        WorkPlanCompletionAssessment::AwaitingUserJudgment { reasons } => {
            ArbiterDecision::NeedsUserJudgment(bounded_text(&reasons.join("; "), 500))
        }
    }
}

/// Terminal-answer check for an ordinary turn-scoped plan.
///
/// `Ok(None)` means no active plan (legacy path). Otherwise returns the
/// arbiter decision; callers must not mark the turn complete when the
/// decision is `Continue`, `Wait`, `Blocked`, or `Inconclusive`.
pub async fn check_ordinary_completion(
    pool: &SqlitePool,
    session_id: &str,
) -> Result<Option<ArbiterDecision>, String> {
    match assess_active_plan(pool, session_id).await? {
        None => Ok(None),
        Some((_plan, _items, assessment)) => Ok(Some(decide_from_assessment(&assessment))),
    }
}

/// Goal-bound completion gate.
///
/// Returns the WorkPlan decision when a bound plan exists and does not allow
/// completion; otherwise returns `AllowCompletion` so the caller proceeds to
/// the authoritative `GoalVerificationService` path. When the verifier later
/// returns `NotMet`, the caller may record the bounded feedback as an
/// actionable WorkItem via [`record_verifier_feedback`] when a bounded
/// existing-item mapping exists.
pub async fn check_goal_completion_gate(
    pool: &SqlitePool,
    goal_id: &str,
) -> Result<ArbiterDecision, String> {
    match assess_goal_plan(pool, goal_id).await? {
        None => Ok(ArbiterDecision::AllowCompletion),
        Some((_plan, _items, assessment)) => Ok(decide_from_assessment(&assessment)),
    }
}

/// Record bounded verifier `NotMet` feedback as plan state when a bounded
/// existing-item mapping exists.
///
/// Prefers the plan's `current_item_id` when it is still actionable, else the
/// first actionable item. Updates only `next_action` (bounded) through CAS so
/// host evidence, dependencies, and objective provenance are untouched. When
/// no actionable item exists, returns `Ok(false)` without mutating the plan;
/// the caller returns the bounded verifier feedback directly.
pub async fn record_verifier_feedback(
    pool: &SqlitePool,
    plan: &WorkPlan,
    items: &[WorkItem],
    next_action: &str,
) -> Result<bool, String> {
    let bounded_action: String = next_action.chars().take(200).collect();
    if bounded_action.trim().is_empty() {
        return Ok(false);
    }
    let target = plan
        .current_item_id
        .as_ref()
        .and_then(|id| items.iter().find(|item| item.id == *id))
        .filter(|item| {
            matches!(
                item.status,
                codegg_core::work_plan::WorkItemStatus::Pending
                    | codegg_core::work_plan::WorkItemStatus::Actionable
                    | codegg_core::work_plan::WorkItemStatus::InProgress
            )
        })
        .or_else(|| {
            codegg_core::work_plan::actionable_items(items)
                .into_iter()
                .find_map(|actionable| items.iter().find(|item| item.id == actionable.id))
        });
    let Some(target) = target else {
        return Ok(false);
    };
    let store = WorkPlanStore::new(pool.clone());
    let patch = codegg_core::work_plan::model::WorkItemPatch {
        next_action: Some(Some(bounded_action)),
        ..Default::default()
    };
    match store.update_item(&target.id, target.revision, patch).await {
        Ok(_) => Ok(true),
        Err(codegg_core::work_plan::WorkPlanError::Conflict { .. }) => Ok(false),
        Err(codegg_core::work_plan::WorkPlanError::Terminal(_)) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

/// Close a turn-scoped plan after the host arbiter passes.
///
/// Transitions the plan to `Completed` through CAS only when the assessment
/// is `Complete` and budgets have not expired. When budgets expired first,
/// remaining state is preserved and the plan is left untouched so a later
/// turn can resume from durable revisions.
pub async fn maybe_complete_plan_on_turn_end(
    pool: &SqlitePool,
    plan: &WorkPlan,
    assessment: &WorkPlanCompletionAssessment,
    budget_expired: bool,
) -> Result<bool, String> {
    if budget_expired {
        return Ok(false);
    }
    if !matches!(assessment, WorkPlanCompletionAssessment::Complete { .. }) {
        return Ok(false);
    }
    let store = WorkPlanStore::new(pool.clone());
    match store
        .transition_plan(
            &plan.id,
            plan.revision,
            codegg_core::work_plan::WorkPlanStatus::Completed,
        )
        .await
    {
        Ok(_) => Ok(true),
        Err(codegg_core::work_plan::WorkPlanError::Conflict { .. }) => Ok(false),
        Err(codegg_core::work_plan::WorkPlanError::Terminal(_)) => Ok(false),
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_work_message_is_bounded() {
        let id = codegg_core::work_plan::WorkItemId("wi_1".to_string());
        let message =
            build_required_work_message(&id, &"x".repeat(2000), 3, Some(&"y".repeat(2000)));
        assert!(message.len() <= MAX_ARBITER_MESSAGE_CHARS + 64);
        assert!(message.contains("wi_1"));
        assert!(message.contains("Next action"));
    }

    #[test]
    fn complete_allows_goal_verifier_path() {
        let decision = decide_from_assessment(&WorkPlanCompletionAssessment::Complete {
            reason: "done".to_string(),
        });
        assert_eq!(decision, ArbiterDecision::AllowCompletion);
    }

    #[test]
    fn actionable_requires_continuation() {
        let decision =
            decide_from_assessment(&WorkPlanCompletionAssessment::ActionableWorkRemaining {
                current_item_id: codegg_core::work_plan::WorkItemId("wi_1".to_string()),
                description: "work".to_string(),
                unmet_count: 1,
                next_action: None,
            });
        assert!(decision.should_continue());
    }
}
