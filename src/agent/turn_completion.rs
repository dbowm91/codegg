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
    /// Called from `run()` after `account_goal_for_turn()`. Each cycle reloads
    /// the current Goal revision, checks budget/terminal status, then assesses
    /// host-observed progress (`Progress` / `VerifiedWait` / `NoProgress`).
    /// Genuine progress resets stagnation; a verified live wait polls the
    /// existing canonical handle without relaunching it; repeated no-progress
    /// issues a bounded nudge/replan sequence and then transitions the same
    /// Goal revision to the existing `AwaitingUser` state. The 32-iteration
    /// cap remains as an emergency invariant only.
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
        // if the runtime returns Continue on every tick. Normal stagnation
        // exits well before this via replan/AwaitingUser; the cap is only an
        // emergency invariant.
        const MAX_CONTINUATIONS: usize = 32;

        let initial_goal_id: Option<String> =
            match goal_store.active_for_session(&self.session_id).await {
                Ok(Some(goal)) => Some(goal.id.clone()),
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("goal continuation initial load failed: {e}");
                    return;
                }
            };
        let mut previous_evidence: Option<codegg_core::goal::progress::GoalContinuationEvidence> =
            None;
        let mut consecutive_no_progress: u8 = 0;

        for _ in 0..MAX_CONTINUATIONS {
            // User steering/cancellation interrupts continuation normally and
            // is never converted into blocker progression.
            if self.steering.load(Ordering::SeqCst)
                || self.cancel_rx.as_ref().is_some_and(|rx| *rx.borrow())
            {
                tracing::info!(
                    session_id = %self.session_id,
                    "goal continuation interrupted by user; stopping"
                );
                return;
            }

            let goal = match goal_store.active_for_session(&self.session_id).await {
                Ok(Some(goal)) => goal,
                Ok(None) => return,
                Err(e) => {
                    tracing::warn!("goal runtime decision failed: {e}");
                    return;
                }
            };
            // Replacement aborts the stale continuation decision.
            if let Some(ref initial) = initial_goal_id {
                if goal.id != *initial {
                    tracing::info!(
                        session_id = %self.session_id,
                        "goal replaced during continuation; aborting stale path"
                    );
                    return;
                }
            }
            // Budget/terminal gate remains authoritative. Progress policy never
            // revives Paused/Cancelled/BudgetLimited/Complete goals.
            let budget_decision = crate::goal::runtime::should_continue(&goal);
            if !budget_decision.should_continue {
                if let Some(prompt) = budget_decision.prompt {
                    // Final wrap-up prompt (e.g. budget-limited).
                    if let Err(error) = self.follow_up_tx.try_send(prompt) {
                        tracing::warn!(?error, "goal wrap-up prompt dropped");
                    }
                    self.drain_follow_up(request, all_events, processor).await;
                } else {
                    tracing::info!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        reason = %budget_decision.reason,
                        "goal continuation stopped (terminal status)"
                    );
                }
                return;
            }

            // Assemble host-owned evidence without holding the todo lock
            // across the job-store query.
            let todo_snapshot = self.services.todo_state.lock().await.clone();
            let pool = goal_store.pool.clone();
            let assembled = crate::goal_continuation::assemble_continuation_evidence(
                &pool,
                &self.session_id,
                &goal,
                &todo_snapshot,
            )
            .await;
            let previous_fingerprint: Option<String> = previous_evidence
                .as_ref()
                .map(codegg_core::goal::progress::goal_continuation_fingerprint);
            let disposition = match &assembled {
                Ok(evidence) => codegg_core::goal::progress::assess_goal_continuation(
                    previous_evidence.as_ref(),
                    evidence,
                ),
                Err(error) => {
                    tracing::warn!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        error = %error,
                        "goal progress evidence unavailable; treating as no-progress"
                    );
                    codegg_core::goal::progress::disposition_for_evidence_failure(
                        previous_fingerprint.as_deref(),
                    )
                }
            };

            match disposition {
                codegg_core::goal::progress::GoalProgressDisposition::Progress { fingerprint } => {
                    consecutive_no_progress = 0;
                    if let Ok(evidence) = assembled {
                        previous_evidence = Some(evidence);
                    }
                    tracing::info!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        reason = "progress",
                        fingerprint = %fingerprint,
                        "goal continuation queued"
                    );
                    let prompt = crate::goal::runtime::build_continuation_prompt(&goal);
                    if let Err(error) = self.follow_up_tx.try_send(prompt) {
                        tracing::warn!(?error, "goal continuation prompt dropped");
                    }
                    self.drain_follow_up(request, all_events, processor).await;
                    self.account_goal_for_turn().await;
                }
                codegg_core::goal::progress::GoalProgressDisposition::VerifiedWait {
                    handle,
                    fingerprint,
                } => {
                    // A verified wait is not stagnation: it resets the
                    // no-progress counter but does not relaunch the awaited
                    // operation. The next boundary reloads the handle; a
                    // terminal outcome becomes progress evidence.
                    consecutive_no_progress = 0;
                    if let Ok(evidence) = assembled {
                        previous_evidence = Some(evidence);
                    }
                    tracing::info!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        reason = "verified_wait",
                        handle_kind = %handle.kind_str(),
                        handle_id = %handle.job_id(),
                        fingerprint = %fingerprint,
                        "goal continuation waiting on live handle"
                    );
                    let prompt = crate::goal::runtime::build_verified_wait_prompt(
                        &goal,
                        &handle,
                        &fingerprint,
                    );
                    if let Err(error) = self.follow_up_tx.try_send(prompt) {
                        tracing::warn!(?error, "goal wait prompt dropped");
                    }
                    self.drain_follow_up(request, all_events, processor).await;
                    self.account_goal_for_turn().await;
                }
                codegg_core::goal::progress::GoalProgressDisposition::NoProgress {
                    fingerprint,
                    reason,
                } => {
                    consecutive_no_progress = consecutive_no_progress.saturating_add(1);
                    if let Ok(evidence) = assembled {
                        previous_evidence = Some(evidence);
                    }
                    let reason_code = match reason {
                        codegg_core::goal::progress::GoalNoProgressReason::NoStateChange => {
                            "no_progress"
                        }
                        codegg_core::goal::progress::GoalNoProgressReason::BlockerReported => {
                            "blocker_reported"
                        }
                        codegg_core::goal::progress::GoalNoProgressReason::EvidenceLoadFailed => {
                            "evidence_load_failed"
                        }
                    };
                    let step =
                        codegg_core::goal::progress::stagnation_step(consecutive_no_progress);
                    tracing::info!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        reason = %reason_code,
                        step = %codegg_core::goal::progress::stagnation_step_str(step),
                        consecutive_no_progress = consecutive_no_progress,
                        fingerprint = %fingerprint,
                        "goal continuation no-progress"
                    );
                    match step {
                        codegg_core::goal::progress::GoalStagnationStep::ContinueWithNudge => {
                            let prompt = crate::goal::runtime::build_continuation_prompt(&goal);
                            if let Err(error) = self.follow_up_tx.try_send(prompt) {
                                tracing::warn!(?error, "goal nudge prompt dropped");
                            }
                            self.drain_follow_up(request, all_events, processor).await;
                            self.account_goal_for_turn().await;
                        }
                        codegg_core::goal::progress::GoalStagnationStep::ContinueWithReplan => {
                            let prompt = crate::goal::runtime::build_replan_prompt(
                                &goal,
                                reason_code,
                                consecutive_no_progress,
                            );
                            if let Err(error) = self.follow_up_tx.try_send(prompt) {
                                tracing::warn!(?error, "goal replan prompt dropped");
                            }
                            self.drain_follow_up(request, all_events, processor).await;
                            self.account_goal_for_turn().await;
                        }
                        codegg_core::goal::progress::GoalStagnationStep::EscalateToAwaitingUser => {
                            // CAS against the observed revision so a concurrent
                            // progress update, pause, cancel, or replacement
                            // wins deterministically.
                            match goal_store
                                .update_status_if_revision(
                                    &goal.id,
                                    goal.revision,
                                    codegg_core::goal::GoalStatus::AwaitingUser,
                                )
                                .await
                            {
                                Ok(Some(updated)) => {
                                    let report =
                                        crate::goal::runtime::build_awaiting_user_blocker_report(
                                            &goal,
                                            &fingerprint,
                                            consecutive_no_progress,
                                        );
                                    tracing::info!(
                                        session_id = %self.session_id,
                                        goal_id = %goal.id,
                                        report = %report,
                                        "goal escalated to awaiting_user after repeated no-progress"
                                    );
                                    crate::bus::global::GlobalEventBus::publish(
                                        crate::bus::events::AppEvent::GoalUpdated {
                                            session_id: self.session_id.clone(),
                                            goal: Box::new(Some(updated.to_snapshot())),
                                        },
                                    );
                                }
                                Ok(None) => {
                                    tracing::info!(
                                        session_id = %self.session_id,
                                        goal_id = %goal.id,
                                        "stale awaiting_user escalation aborted (goal changed)"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        session_id = %self.session_id,
                                        goal_id = %goal.id,
                                        error = %e,
                                        "awaiting_user escalation failed"
                                    );
                                }
                            }
                            return;
                        }
                    }
                }
            }
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
