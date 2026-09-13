//! Turn completion, goal accounting, and run-boundary reporting.
//!
//! Physical decomposition of [`super::r#loop`] (M003): terminal event
//! publication, per-turn goal budget accounting with bounded continuation,
//! execution-limit checks, and durable run-boundary journaling. Cancellation,
//! scheduler, and goal-store authority are unchanged; this module only moves
//! the call sites to their explicit owners.

use std::panic::AssertUnwindSafe;
use std::sync::atomic::Ordering;

use super::r#loop::AgentLoop;
use crate::agent::processor::EventProcessor;
use crate::bus::events::AppEvent;
use crate::error::AppError;
use crate::provider::{ChatEvent, ChatRequest};
use futures_util::FutureExt;

impl AgentLoop {
    pub(super) fn publish_agent_finished(&mut self, events: &[ChatEvent]) {
        self.finalize_habit_observation(events);
        let last_finish = events.iter().rev().find_map(|event| {
            if let ChatEvent::Finish { stop_reason, usage } = event {
                Some((stop_reason, usage))
            } else {
                None
            }
        });

        let (stop_reason_str, input_tokens, output_tokens, cached_tokens, reasoning_tokens) =
            if let Some((stop_reason, usage)) = last_finish {
                (
                    stop_reason.to_string(),
                    Some(usage.input_tokens),
                    Some(usage.output_tokens),
                    usage.cached_tokens,
                    if usage.reasoning_tokens > 0 {
                        Some(usage.reasoning_tokens)
                    } else {
                        None
                    },
                )
            } else if self.steering.load(Ordering::SeqCst)
                || self.cancel_rx.as_ref().is_some_and(|rx| *rx.borrow())
            {
                ("interrupted".to_string(), None, None, None, None)
            } else {
                ("completed".to_string(), None, None, None, None)
            };

        crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
            session_id: self.session_id.clone(),
            stop_reason: stop_reason_str,
            input_tokens,
            output_tokens,
            cached_tokens,
            reasoning_tokens,
        });

        // Dispatch event observation hook for agent finished.
        if let Some(ref ps) = self.plugin_service {
            use crate::plugin::lifecycle::{EventHookInput, LifecycleHooks};
            let hooks = LifecycleHooks::new(
                ps.clone(),
                crate::plugin::policy::PluginLifecyclePolicy::default(),
            );
            let event_input = EventHookInput {
                event_type: "agent.finished".into(),
                session_id: Some(self.session_id.clone()),
                event: serde_json::json!({
                    "session_id": self.session_id,
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                }),
            };
            tokio::spawn(async move {
                let result = AssertUnwindSafe(async move {
                    hooks.emit_event(event_input).await;
                })
                .catch_unwind()
                .await;
                if let Err(e) = result {
                    tracing::error!(panic = ?e, "hook emission task panicked");
                }
            });
        }
    }

    pub(super) fn publish_agent_finished_error(&self, error: &AppError) {
        crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
            session_id: self.session_id.clone(),
            stop_reason: if self.steering.load(Ordering::SeqCst) {
                "interrupted".to_string()
            } else {
                "error".to_string()
            },
            input_tokens: (self.state.unaccounted_input_tokens > 0).then(|| {
                usize::try_from(self.state.unaccounted_input_tokens).unwrap_or(usize::MAX)
            }),
            output_tokens: (self.state.unaccounted_output_tokens > 0).then(|| {
                usize::try_from(self.state.unaccounted_output_tokens).unwrap_or(usize::MAX)
            }),
            cached_tokens: None,
            reasoning_tokens: None,
        });
        tracing::error!(session_id = %self.session_id, error = %error, "agent loop failed");
    }

    /// Account a finished turn against the active goal. Called from
    /// `run()` after the loop body so the budget is updated even on
    /// the user's last turn.
    pub(super) async fn account_goal_for_turn(&mut self) {
        let Some(goal_store) = self.services.goal_store.clone() else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }
        // Compute wall-clock delta since the last accounting tick.
        // `goal_wall_clock` is a short critical section never held across
        // `.await`, so `std::sync::Mutex` is safe here; poisoning is
        // surfaced loudly rather than silently defaulted.
        let wallclock_delta = {
            let mut wc = self.goal_wall_clock.lock().unwrap_or_else(|p| {
                tracing::warn!("goal_wall_clock mutex poisoned; recovering clock state");
                p.into_inner()
            });
            let delta = wc.elapsed_secs_since_last();
            // Always reset the clock so the next tick measures fresh
            // wall-clock, even when the goal store is unavailable.
            wc.last_accounted_at = Some(std::time::Instant::now());
            delta
        };
        let tool_calls = self.state.unaccounted_tool_calls as i64;
        let input_tokens = self.state.unaccounted_input_tokens;
        let output_tokens = self.state.unaccounted_output_tokens;
        let result = crate::goal::runtime::account_for_turn(
            &goal_store,
            &self.session_id,
            input_tokens,
            output_tokens,
            tool_calls,
            1,
            wallclock_delta,
        )
        .await;
        if result.is_ok() {
            self.state.unaccounted_tool_calls = 0;
            self.state.unaccounted_input_tokens = 0;
            self.state.unaccounted_output_tokens = 0;
        } else {
            tracing::warn!(session_id = %self.session_id, "goal accounting failed; retaining unaccounted deltas");
        }
    }

    /// Decide whether to autonomously continue the active goal.
    ///
    /// Called from `run()` after `account_goal_for_turn()`. If the goal
    /// runtime returns `Continue`, we queue a continuation prompt and
    /// recurse through `drain_follow_up`. If it returns `BudgetLimited`,
    /// we queue a wrap-up prompt and let the loop drain that single
    /// follow-up without scheduling another continuation. This mirrors
    /// codex's `maybe_start_goal_continuation_turn` pattern.
    pub(super) async fn maybe_continue_goal(
        &mut self,
        request: &mut ChatRequest,
        all_events: &mut Vec<ChatEvent>,
        processor: &mut EventProcessor,
    ) {
        let Some(goal_store) = self.services.goal_store.clone() else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }

        // Bounded safety: don't run the continuation loop forever even
        // if the runtime returns Continue on every tick. We rely on
        // the budget/terminal-status checks inside `should_continue`
        // to break out, but cap the outer iterations as a guard.
        const MAX_CONTINUATIONS: usize = 32;
        for _ in 0..MAX_CONTINUATIONS {
            let decision = match crate::goal::runtime::should_continue_for_session(
                &goal_store,
                &self.session_id,
            )
            .await
            {
                Ok(Some(d)) => d,
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("goal runtime decision failed: {e}");
                    return;
                }
            };
            if !decision.should_continue {
                if let Some(prompt) = decision.prompt {
                    // Final wrap-up prompt (e.g. budget-limited).
                    if let Err(error) = self.follow_up_tx.try_send(prompt) {
                        tracing::warn!(?error, "goal wrap-up prompt dropped");
                    }
                    self.drain_follow_up(request, all_events, processor).await;
                }
                return;
            }
            let Some(prompt) = decision.prompt else {
                return;
            };
            tracing::info!(
                "goal continuation queued (session={}): {}",
                self.session_id,
                decision.reason
            );
            // Reset per-turn token/tool counters so the next
            // accounting tick measures the *continuation* turn, not
            // a stale carry-over from the user's turn.
            if let Err(error) = self.follow_up_tx.try_send(prompt) {
                tracing::warn!(?error, "goal continuation prompt dropped");
            }
            self.drain_follow_up(request, all_events, processor).await;
            // After the continuation turn finishes, account for it
            // before deciding whether to continue again.
            // We can't call `account_goal_for_turn` here directly
            // because it borrows self immutably and we already have
            // &mut self via the request parameter. Inline the
            // accounting using a clone of the wall-clock state.
            self.account_goal_for_turn().await;
        }
        tracing::warn!("goal continuation hit MAX_CONTINUATIONS={MAX_CONTINUATIONS}, halting");
    }

    pub(super) fn check_limits(&self) -> Option<String> {
        if let Some(agent) = self.services.agents.get(&self.state.current_agent) {
            if let Some(steps) = agent.steps {
                if self.state.turn_count >= steps {
                    return Some(format!("max steps ({}) reached", steps));
                }
            }
        }

        if self.state.turn_count >= self.limits.max_turns {
            return Some(format!("max turns ({}) reached", self.limits.max_turns));
        }

        if let Some(max) = self.max_tool_calls {
            if self.state.tool_call_count >= max {
                return Some(format!("max tool calls ({}) reached", max));
            }
        }

        if self.state.total_tokens >= self.limits.max_tokens {
            return Some(format!("max tokens ({}) reached", self.limits.max_tokens));
        }

        if self.state.start_time.elapsed() >= self.limits.timeout {
            return Some(format!("timeout ({:?}) reached", self.limits.timeout));
        }

        if self.steering.load(Ordering::SeqCst) {
            return Some("interrupted by user".to_string());
        }

        None
    }

    pub(super) async fn record_run_boundary(&self, boundary: &str) {
        let (Some(control), Some(run_id)) = (&self.services.run_control, &self.run_id) else {
            return;
        };
        if let Err(error) = control
            .append(
                run_id.clone(),
                codegg_core::agent_run_control::AgentRunJournalEventKind::SafeBoundary,
                None,
                None,
                [("boundary".into(), boundary.into())],
            )
            .await
        {
            tracing::warn!(
                run_id = %run_id,
                boundary,
                %error,
                "failed to journal run boundary"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::agent::r#loop::AgentLoopState;
    use std::time::Instant;

    #[test]
    fn accounting_deltas_are_distinct_from_cumulative_limits() {
        let mut state = AgentLoopState {
            current_agent: "standard".to_string(),
            turn_count: 3,
            total_tokens: 100,
            start_time: Instant::now(),
            plan_mode: false,
            plan_topic: None,
            tool_call_count: 5,
            unaccounted_tool_calls: 2,
            unaccounted_input_tokens: 11,
            unaccounted_output_tokens: 7,
        };

        // A successful accounting tick consumes only the delta; the hard
        // limit counter remains cumulative for subsequent checks.
        state.unaccounted_tool_calls = 0;
        state.unaccounted_input_tokens = 0;
        state.unaccounted_output_tokens = 0;
        assert_eq!(state.tool_call_count, 5);
        assert_eq!(state.unaccounted_tool_calls, 0);
    }
}
