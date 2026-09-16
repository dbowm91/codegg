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

use crate::bus::events::{AppEvent, GoalBudgetSnapshot, GoalSnapshot, GoalUsageSnapshot};
use crate::bus::global::GlobalEventBus;
use crate::error::AppError;
use crate::goal::model::Goal;
use crate::goal::store::GoalStore;
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
            format!(
                "Tokens used: {} / {} ({} remaining)",
                used,
                t,
                t.saturating_sub(used)
            )
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

    let phase = goal.current_phase.as_deref().unwrap_or("(not yet started)");

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
  `open_questions` populated with the concrete blocker evidence and the
  next action you need. The host tracks repeated no-progress turns and
  moves the goal to the existing `AwaitingUser` state when the blocker
  remains unresolved. There is no `Blocked` goal status.
- Report open questions and blocker evidence through `goal_update_progress`;
  narration alone without host-observable state change does not count as
  progress.
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
        next = goal.next_action.as_deref().unwrap_or("(none recorded)"),
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

/// Continuation prompt for a verified live wait.
///
/// The named handle is host-owned and already running. The model must poll
/// the existing handle through the established job/run mechanism and must
/// not relaunch the operation.
pub fn build_verified_wait_prompt(
    goal: &Goal,
    handle: &super::progress::WaitHandleRef,
    fingerprint: &str,
) -> String {
    let bounded_id: String = handle.job_id().chars().take(128).collect();
    format!(
        r#"## Goal Verified Wait

The active goal "{title}" has a live host-owned operation that is still
running. Do not start a duplicate operation.

- Wait handle: {kind} {id}
- Progress fingerprint: {fingerprint}
- Poll the existing handle using the established job/run mechanism and
  report its durable outcome. Repeating narration without host-observable
  state change does not count as progress.

Objective: {objective}
"#,
        title = goal.title,
        kind = handle.kind_str(),
        id = bounded_id,
        fingerprint = fingerprint,
        objective = goal.objective,
    )
}

/// Replan prompt issued after repeated no-progress turns and before the
/// terminal `AwaitingUser` handoff. Bounded and free of command output.
pub fn build_replan_prompt(goal: &Goal, reason: &str, consecutive_no_progress: u8) -> String {
    let bounded_reason: String = reason.chars().take(240).collect();
    format!(
        r#"## Goal Replan Required

The last {count} continuation turn(s) produced no host-observable progress
for goal "{title}" (reason: {reason}).

- Re-read the objective, success criteria, and the latest host-owned
  evidence (todos, test/delegated-run records).
- Propose a different next action via `goal_update_progress`. If you are
  blocked, populate `open_questions` with the concrete blocker evidence
  and the exact user input needed.
- Do not repeat the same calls with the same arguments. Narration alone
  does not count as progress.

Objective: {objective}
"#,
        count = consecutive_no_progress,
        title = goal.title,
        reason = bounded_reason,
        objective = goal.objective,
    )
}

/// Concise blocker report recorded when stagnation escalates to
/// `AwaitingUser`. Contains only bounded metadata, never raw tool output or
/// model reasoning.
pub fn build_awaiting_user_blocker_report(
    goal: &Goal,
    fingerprint: &str,
    consecutive_no_progress: u8,
) -> String {
    let open_count = goal.open_questions.len();
    format!(
        "goal '{title}' entered awaiting_user after {count} consecutive no-progress turn(s); \
fingerprint {fingerprint}; open_questions {open_count}; next_action '{next}'",
        title = goal.title.chars().take(120).collect::<String>(),
        count = consecutive_no_progress,
        fingerprint = fingerprint,
        open_count = open_count,
        next = goal
            .next_action
            .as_deref()
            .unwrap_or("(none recorded)")
            .chars()
            .take(160)
            .collect::<String>(),
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
            revision: 0,
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
    fn should_continue_blocks_on_each_budget_axis() {
        let mut budget = GoalBudget {
            max_tool_calls: Some(2),
            ..Default::default()
        };
        let mut usage = GoalUsage {
            tool_calls: 2,
            ..Default::default()
        };
        assert!(!should_continue(&test_goal(budget, usage)).should_continue);

        budget = GoalBudget {
            max_turns: Some(1),
            ..Default::default()
        };
        usage = GoalUsage {
            turns_used: 1,
            ..Default::default()
        };
        let d = should_continue(&test_goal(budget, usage));
        assert!(!d.should_continue);
        assert!(d.reason.contains("turn budget"));

        budget = GoalBudget {
            max_wallclock_secs: Some(10),
            ..Default::default()
        };
        usage = GoalUsage {
            wallclock_secs: 10,
            ..Default::default()
        };
        let d = should_continue(&test_goal(budget, usage));
        assert!(!d.should_continue);
        assert!(d.reason.contains("wall-clock budget"));
    }

    #[test]
    fn should_continue_never_revives_non_active_status() {
        for status in [
            GoalStatus::Paused,
            GoalStatus::AwaitingUser,
            GoalStatus::BudgetLimited,
            GoalStatus::Cancelled,
            GoalStatus::Failed,
        ] {
            let mut g = test_goal(GoalBudget::default(), GoalUsage::default());
            g.status = status;
            assert!(
                !should_continue(&g).should_continue,
                "status must not continue"
            );
        }
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
    fn continuation_prompt_has_no_blocked_status_contract() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let prompt = build_continuation_prompt(&g);
        // Guard against reintroducing the nonexistent `Blocked` goal status
        // contract. The prompt may deny the status ("There is no `Blocked`")
        // but must never offer it ("allowing a `Blocked` status") or describe
        // consecutive-blocked counting. `TodoStatus::Blocked` is a separate
        // todo-level state and must not be presented as a goal status here.
        assert!(
            !prompt.contains("allowing a `Blocked`"),
            "continuation prompt must not offer a `Blocked` goal status"
        );
        assert!(
            !prompt
                .to_lowercase()
                .contains("consecutive blocked turns before"),
            "continuation prompt must not describe consecutive-blocked goal behavior"
        );
        assert!(
            prompt.contains("AwaitingUser"),
            "continuation prompt must direct blockers to the existing AwaitingUser state"
        );
        assert!(
            prompt.contains("There is no `Blocked`"),
            "continuation prompt must explicitly deny the Blocked goal status"
        );
    }

    #[test]
    fn verified_wait_prompt_names_handle_without_relaunch() {
        use crate::goal::progress::WaitHandleRef;
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let handle = WaitHandleRef::TestJob {
            job_id: "job-123".into(),
        };
        let prompt = build_verified_wait_prompt(&g, &handle, "sha256:abc");
        assert!(prompt.contains("job-123"));
        assert!(prompt.contains("Do not start a duplicate"));
        assert!(!prompt.contains("`Blocked`"));
    }

    #[test]
    fn replan_prompt_is_bounded_and_mentions_evidence() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let prompt = build_replan_prompt(&g, "no_state_change", 2);
        assert!(prompt.contains("Replan"));
        assert!(prompt.contains("no_state_change"));
        assert!(!prompt.contains("`Blocked`"));
    }

    #[test]
    fn awaiting_user_report_is_bounded_metadata_only() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let report = build_awaiting_user_blocker_report(&g, "sha256:abc", 3);
        assert!(report.contains("awaiting_user"));
        assert!(report.contains("sha256:abc"));
        assert!(!report.contains("`Blocked`"));
    }

    #[test]
    fn wrap_up_prompt_mentions_budget() {
        let g = test_goal(GoalBudget::default(), GoalUsage::default());
        let p = build_budget_wrap_up_prompt(&g, "token budget");
        assert!(p.contains("Budget Reached"));
        assert!(p.contains("token budget"));
    }
}
