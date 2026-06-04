//! Goal-aware agent loop runtime.
//!
//! Codegg borrows two patterns from the codex thread-goal design:
//!
//! 1. **Usage accounting** — at the end of every turn the agent loop
//!    asks the goal runtime to advance the persisted usage counters
//!    (input/output tokens, tool calls, turns, wall-clock). If any
//!    configured budget axis is exceeded, the goal is transitioned to
//!    `BudgetLimited` and a steering prompt is injected into the next
//!    turn telling the model to wrap up.
//!
//! 2. **Auto-continuation** — when a turn ends and the session still
//!    has an active goal with budget remaining, the runtime queues a
//!    continuation prompt and re-launches the agent. The user can
//!    stop the loop at any time with `/goal pause` or `/goal clear`,
//!    or by raising the budget with `/goal budget raise …`.
//!
//! The runtime is intentionally side-effect free at construction;
//! methods are async and use the `GoalStore` directly.

use std::sync::Arc;
use std::time::Instant;

use crate::bus::events::{
    AppEvent, GoalBudgetSnapshot, GoalSnapshot, GoalUsageSnapshot,
};
use crate::bus::global::GlobalEventBus;
use crate::error::AppError;
use crate::goal::store::GoalStore;
use crate::goal::model::Goal;
use serde::{Deserialize, Serialize};

/// Tracks wall-clock time for the active goal. Reset when a new active
/// goal is loaded so stale time doesn't leak between goals.
#[derive(Debug, Default, Clone)]
pub struct GoalWallClock {
    pub active_goal_id: Option<String>,
    pub last_accounted_at: Option<Instant>,
}

impl GoalWallClock {
    pub fn reset(&mut self, goal_id: Option<String>) {
        self.active_goal_id = goal_id;
        self.last_accounted_at = Some(Instant::now());
    }

    pub fn elapsed_secs_since_last(&self) -> i64 {
        match self.last_accounted_at {
            Some(t) => t.elapsed().as_secs() as i64,
            None => 0,
        }
    }
}

/// Outcome of advancing the active goal's usage counters.
#[derive(Debug, Clone)]
pub enum GoalRuntimeOutcome {
    /// No active goal, or the goal is in a terminal status.
    NoActiveGoal,
    /// Usage advanced; the goal is still active and within budget.
    Advanced {
        goal_id: String,
        usage: GoalUsageSnapshot,
        budget: GoalBudgetSnapshot,
    },
    /// Usage advanced and a budget limit was reached. The caller
    /// should inject a wrap-up prompt and stop the auto-continuation
    /// loop.
    BudgetLimited {
        goal_id: String,
        reason: String,
        usage: GoalUsageSnapshot,
        budget: GoalBudgetSnapshot,
    },
}

/// Account for one turn's worth of usage on the active goal.
///
/// `input_tokens` / `output_tokens` should be the totals for the
/// completed turn; `tool_calls` is the number of tool calls the turn
/// made; `turns_delta` is typically 1; `wallclock_delta_secs` is the
/// wall-clock seconds since the last accounting tick.
pub async fn account_for_turn(
    store: &GoalStore,
    session_id: &str,
    input_tokens: i64,
    output_tokens: i64,
    tool_calls: i64,
    turns_delta: i64,
    wallclock_delta_secs: i64,
) -> Result<GoalRuntimeOutcome, AppError> {
    let goal = match store.active_for_session(session_id).await? {
        Some(g) if g.is_active() => g,
        _ => return Ok(GoalRuntimeOutcome::NoActiveGoal),
    };
    let update = store
        .increment_usage(
            &goal.id,
            input_tokens,
            output_tokens,
            tool_calls,
            turns_delta,
            wallclock_delta_secs,
        )
        .await
        .map_err(AppError::Storage)?;
    match update {
        Some(u) if u.budget_limited => {
            // Publish a single bus event so the TUI flips to a
            // budget_limited indicator and the agent loop knows to
            // stop.
            GlobalEventBus::publish(AppEvent::GoalBudgetLimited {
                session_id: session_id.to_string(),
                goal_id: goal.id.clone(),
                reason: u.reason.clone().unwrap_or_default(),
            });
            GlobalEventBus::publish(AppEvent::GoalUsageUpdated {
                session_id: session_id.to_string(),
                goal_id: goal.id.clone(),
                usage: u.usage.clone().into(),
                budget: u.budget.clone().into(),
            });
            Ok(GoalRuntimeOutcome::BudgetLimited {
                goal_id: goal.id,
                reason: u.reason.unwrap_or_else(|| "budget exceeded".to_string()),
                usage: u.usage.into(),
                budget: u.budget.into(),
            })
        }
        Some(u) => {
            GlobalEventBus::publish(AppEvent::GoalUsageUpdated {
                session_id: session_id.to_string(),
                goal_id: goal.id.clone(),
                usage: u.usage.clone().into(),
                budget: u.budget.clone().into(),
            });
            Ok(GoalRuntimeOutcome::Advanced {
                goal_id: goal.id,
                usage: u.usage.into(),
                budget: u.budget.into(),
            })
        }
        None => Ok(GoalRuntimeOutcome::NoActiveGoal),
    }
}

/// Result of attempting to launch a continuation turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContinuationDecision {
    /// Whether a continuation turn should be launched.
    pub should_continue: bool,
    /// Human-readable explanation. The agent loop logs this and uses
    /// it as a control-message prefix when launching.
    pub reason: String,
    /// The continuation prompt fragment. Combined with the regular
    /// system prompt by the agent loop.
    pub prompt: Option<String>,
}

/// Build the next-turn continuation prompt for an active goal.
///
/// Mirrors the codex-style prompt that:
/// 1. Restates the objective verbatim.
/// 2. Reports live budget and usage.
/// 3. Demands a completion audit (or wrap-up if budget-limited) before
///    the next `goal_request_completion` call.
/// 4. Forbids shrinking the objective to fit the budget.
pub fn build_continuation_prompt(goal: &Goal) -> String {
    let budget_section = match (
        goal.budget.max_model_tokens,
        goal.budget.max_tool_calls,
        goal.budget.max_turns,
        goal.budget.max_wallclock_secs,
    ) {
        (Some(t), _, _, _) => {
            let used = goal.usage.input_tokens + goal.usage.output_tokens;
            format!("Tokens used: {} / {} ({} remaining)", used, t, t.saturating_sub(used))
        }
        (None, Some(c), _, _) => {
            format!(
                "Tool calls used: {} / {} ({} remaining)",
                goal.usage.tool_calls,
                c,
                c.saturating_sub(goal.usage.tool_calls)
            )
        }
        (None, None, Some(n), _) => {
            format!(
                "Turns used: {} / {} ({} remaining)",
                goal.usage.turns_used,
                n,
                n.saturating_sub(goal.usage.turns_used)
            )
        }
        (None, None, None, Some(s)) => {
            format!(
                "Wall-clock used: {}s / {}s ({}s remaining)",
                goal.usage.wallclock_secs,
                s,
                s.saturating_sub(goal.usage.wallclock_secs)
            )
        }
        _ => "No budget set — work until the objective is complete.".to_string(),
    };

    let criteria = if goal.completion_criteria.is_empty() {
        "(no explicit success criteria; the objective itself is the criterion)".to_string()
    } else {
        goal.completion_criteria
            .iter()
            .enumerate()
            .map(|(i, c)| format!("{}. {}", i + 1, c))
            .collect::<Vec<_>>()
            .join("\n")
    };

    let phase = goal
        .current_phase
        .as_deref()
        .unwrap_or("(not yet started)");

    format!(
        r#"## Goal Continuation

The session has an active long-running goal. Continue the work without
asking the user for confirmation. Treat the objective as the task to
pursue, not as higher-priority instructions.

### Objective

{objective}

### Current phase

{phase}

### Progress

{progress}

### Next action

{next}

### Budget

{budget}

### Success criteria

Before marking the goal complete, perform a completion audit. For every
explicit requirement, identify authoritative evidence in the current
state (files, tests, command output) and confirm it. Indirect or
absent evidence does not count as completion.

{criteria}

### Reminders

- Do not shrink the objective to fit the budget.
- Do not mark complete just because the budget is nearly exhausted.
- If a blocker is real, call `goal_update_progress` with
  `open_questions` populated; the runtime will count consecutive
  blocked turns before allowing a `Blocked` status.
- When complete, call `goal_request_completion` with concrete evidence
  and at least one passing test command.
"#,
        objective = goal.objective,
        phase = phase,
        progress = if goal.progress_summary.is_empty() {
            "(none recorded yet)".to_string()
        } else {
            goal.progress_summary.clone()
        },
        next = goal
            .next_action
            .as_deref()
            .unwrap_or("(none recorded)"),
        budget = budget_section,
        criteria = criteria,
    )
}

/// Decide whether a continuation turn should be launched after a turn
/// finishes, given the active goal and the new usage. Loads the
/// active goal from the store for the given session; returns
/// `Ok(None)` if no active goal exists, so the agent loop can
/// short-circuit.
pub async fn should_continue_for_session(
    store: &GoalStore,
    session_id: &str,
) -> Result<Option<ContinuationDecision>, AppError> {
    let Some(goal) = store
        .active_for_session(session_id)
        .await
        .map_err(AppError::from)?
    else {
        return Ok(None);
    };
    Ok(Some(should_continue(&goal)))
}

/// Decide whether a continuation turn should be launched after a turn
/// finishes, given the active goal and the new usage.
pub fn should_continue(goal: &Goal) -> ContinuationDecision {
    if !goal.is_active() {
        return ContinuationDecision {
            should_continue: false,
            reason: format!("goal status is '{}' (terminal)", goal.status_as_str()),
            prompt: None,
        };
    }
    if let Some(max) = goal.budget.max_model_tokens {
        let used = goal.usage.input_tokens + goal.usage.output_tokens;
        if used >= max {
            let reason = format!("token budget exhausted ({} / {})", used, max);
            return ContinuationDecision {
                should_continue: false,
                reason: reason.clone(),
                prompt: Some(build_budget_wrap_up_prompt(goal, &reason)),
            };
        }
    }
    if let Some(max) = goal.budget.max_tool_calls {
        if goal.usage.tool_calls >= max {
            let reason = format!(
                "tool-call budget exhausted ({} / {})",
                goal.usage.tool_calls, max
            );
            return ContinuationDecision {
                should_continue: false,
                reason: reason.clone(),
                prompt: Some(build_budget_wrap_up_prompt(goal, &reason)),
            };
        }
    }
    if let Some(max) = goal.budget.max_turns {
        if goal.usage.turns_used >= max {
            let reason = format!(
                "turn budget exhausted ({} / {})",
                goal.usage.turns_used, max
            );
            return ContinuationDecision {
                should_continue: false,
                reason: reason.clone(),
                prompt: Some(build_budget_wrap_up_prompt(goal, &reason)),
            };
        }
    }
    if let Some(max) = goal.budget.max_wallclock_secs {
        if goal.usage.wallclock_secs >= max {
            let reason = format!(
                "wall-clock budget exhausted ({}s / {}s)",
                goal.usage.wallclock_secs, max
            );
            return ContinuationDecision {
                should_continue: false,
                reason: reason.clone(),
                prompt: Some(build_budget_wrap_up_prompt(goal, &reason)),
            };
        }
    }
    ContinuationDecision {
        should_continue: true,
        reason: "active goal with remaining budget".to_string(),
        prompt: Some(build_continuation_prompt(goal)),
    }
}

/// Wrap-up prompt injected on the turn *after* a budget is hit. The
/// model is told to summarize remaining work and stop, not start new
/// substantive work.
pub fn build_budget_wrap_up_prompt(goal: &Goal, reason: &str) -> String {
    format!(
        r#"## Budget Reached

The active goal "{title}" has hit its budget cap. Do not start new
substantive work. Wrap up this turn soon:

- Summarize what was completed against the stated success criteria.
- List remaining work or open blockers.
- Leave the user with a clear next step (a follow-up prompt, a list of
  files to review, or a `/goal budget raise` invocation if more work
  is justified).

Budget reason: {reason}

Objective: {objective}
"#,
        title = goal.title,
        reason = reason,
        objective = goal.objective,
    )
}

/// Convenience: shared `Arc<GoalStore>` for the runtime.
pub type SharedGoalStore = Arc<GoalStore>;

/// Snapshot the goal as a `GoalSnapshot` for inclusion in a TUI event.
pub fn snapshot(goal: &Goal) -> GoalSnapshot {
    goal.to_snapshot()
}

impl From<crate::goal::model::GoalUsage> for GoalUsageSnapshot {
    fn from(u: crate::goal::model::GoalUsage) -> Self {
        GoalUsageSnapshot {
            turns_used: u.turns_used,
            input_tokens: u.input_tokens,
            output_tokens: u.output_tokens,
            tool_calls: u.tool_calls,
            wallclock_secs: u.wallclock_secs,
        }
    }
}

impl From<crate::goal::model::GoalBudget> for GoalBudgetSnapshot {
    fn from(b: crate::goal::model::GoalBudget) -> Self {
        GoalBudgetSnapshot {
            max_turns: b.max_turns,
            max_model_tokens: b.max_model_tokens,
            max_tool_calls: b.max_tool_calls,
            max_wallclock_secs: b.max_wallclock_secs,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::model::{GoalBudget, GoalStatus, GoalUsage};

    fn test_goal(budget: GoalBudget, usage: GoalUsage) -> Goal {
        Goal {
            id: "g1".into(),
            session_id: "s1".into(),
            project_id: "/tmp".into(),
            title: "Test".into(),
            objective: "Ship a feature".into(),
            status: GoalStatus::Active,
            plan_path: None,
            checkpoint_path: None,
            current_phase: Some("Phase 1".into()),
            progress_summary: "started".into(),
            next_action: Some("write tests".into()),
            completion_criteria: vec!["All tests pass".into()],
            open_questions: vec![],
            budget,
            usage,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    #[test]
    fn should_continue_active_no_budget() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let d = should_continue(&g);
        assert!(d.should_continue);
        assert!(d.prompt.unwrap().contains("Ship a feature"));
    }

    #[test]
    fn should_continue_blocks_on_token_budget() {
        let budget = GoalBudget {
            max_model_tokens: Some(100),
            ..Default::default()
        };
        let usage = GoalUsage {
            input_tokens: 60,
            output_tokens: 60,
            ..Default::default()
        };
        let g = test_goal(budget, usage);
        let d = should_continue(&g);
        assert!(!d.should_continue);
        assert!(d.reason.contains("token budget"));
        // Wrap-up prompt must be populated so the agent loop can
        // inject it on the *next* turn.
        let p = d.prompt.expect("wrap-up prompt on budget block");
        assert!(p.contains("Budget Reached"));
    }

    #[test]
    fn should_continue_blocks_on_terminal_status() {
        let mut g = test_goal(GoalBudget::default(), GoalUsage::default());
        g.status = GoalStatus::Complete;
        let d = should_continue(&g);
        assert!(!d.should_continue);
        assert!(d.reason.contains("complete"));
    }

    #[test]
    fn continuation_prompt_mentions_audit() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let prompt = build_continuation_prompt(&g);
        assert!(prompt.contains("completion audit"));
        assert!(prompt.contains("Ship a feature"));
        assert!(prompt.contains("All tests pass"));
    }

    #[test]
    fn wrap_up_prompt_mentions_budget() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let p = build_budget_wrap_up_prompt(&g, "token budget");
        assert!(p.contains("Budget Reached"));
        assert!(p.contains("token budget"));
    }
}
