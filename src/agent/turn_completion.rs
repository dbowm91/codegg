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
use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message};
use futures_util::FutureExt;

/// Extract provider-reported usage from a completed turn's events.
///
/// Returns `(stop_reason, input, output, cached, reasoning)`. This is the
/// same source `publish_agent_finished` reads, exposed separately so the
/// daemon can populate usage on `CoreEvent::TurnCompleted` — the only
/// completion event that crosses the process boundary to the TUI.
pub(super) fn turn_usage(
    events: &[ChatEvent],
    retained_stop_reason: Option<&str>,
) -> (
    String,
    Option<usize>,
    Option<usize>,
    Option<usize>,
    Option<usize>,
) {
    let last_finish = events.iter().rev().find_map(|event| {
        if let ChatEvent::Finish { stop_reason, usage } = event {
            Some((stop_reason, usage))
        } else {
            None
        }
    });

    // A retained reason (a spent turn budget, a terminal classification) wins
    // over the provider's own finish reason: the last response may well have
    // announced tool calls that the loop never executed. A real provider
    // finish with no retained reason keeps its own reason, unchanged.
    let stop_reason = retained_stop_reason
        .map(str::to_owned)
        .or_else(|| last_finish.map(|(reason, _)| reason.to_string()))
        .unwrap_or_else(|| "completed".to_string());

    match last_finish.map(|(_, usage)| usage) {
        Some(usage) => (
            stop_reason,
            Some(usage.input_tokens),
            Some(usage.output_tokens),
            usage.cached_tokens,
            if usage.reasoning_tokens > 0 {
                Some(usage.reasoning_tokens)
            } else {
                None
            },
        ),
        None => (stop_reason, None, None, None, None),
    }
}

/// Whether a published stop reason names a spent execution budget.
///
/// The strings are the ones [`AgentLoop::check_limits`] produces. Consumers
/// that branch on a *kind* (the daemon notification bridge) must classify
/// these as "not finished", not as an ordinary completion; matching the
/// reason text here keeps one vocabulary instead of a second enum.
pub(crate) fn is_budget_stop_reason(reason: &str) -> bool {
    const BUDGET_REASONS: [&str; 5] = [
        "max turns",
        "max steps",
        "max tool calls",
        "max tokens",
        "timeout (",
    ];
    BUDGET_REASONS
        .iter()
        .any(|budget| reason.starts_with(budget))
}

/// Whether a limit stop may spend one final provider call to summarize.
///
/// `max steps` already injected its tool-stripped termination instruction on
/// the iteration before the break, and the timeout check only returns once the
/// deadline has elapsed — that call would either duplicate the existing
/// summary or start a provider request after the bound that exists to stop it.
pub(super) fn limit_stop_allows_summary(reason: &str) -> bool {
    reason.starts_with("max turns")
        || reason.starts_with("max tool calls")
        || reason.starts_with("max tokens")
}

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

        // Precedence: an interrupt or cancel is always `interrupted` (even
        // when a provider finish is present), then the reason the loop
        // retained when a limit stopped it, then the provider's own finish,
        // then the literal `completed` for a finish-less success.
        let stop_reason_str = self
            .turn_stop_reason()
            .or_else(|| last_finish.map(|(stop_reason, _)| stop_reason.to_string()))
            .unwrap_or_else(|| "completed".to_string());
        let (input_tokens, output_tokens, cached_tokens, reasoning_tokens) =
            match last_finish.map(|(_, usage)| usage) {
                Some(usage) => (
                    Some(usage.input_tokens),
                    Some(usage.output_tokens),
                    usage.cached_tokens,
                    if usage.reasoning_tokens > 0 {
                        Some(usage.reasoning_tokens)
                    } else {
                        None
                    },
                ),
                None => (None, None, None, None),
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
    ) -> Result<(), crate::error::AppError> {
        let Some(goal_store) = self.services.goal_store.clone() else {
            return Ok(());
        };
        if self.session_id.is_empty() {
            return Ok(());
        }

        // A loop-budget stop is not "the model finished". Continuing would
        // relaunch an identical capped loop, so the continuation decision
        // reads the reason the loop retained instead of guessing.
        if let Some(reason) = self.turn_stop_reason() {
            tracing::info!(
                session_id = %self.session_id,
                reason = %reason,
                "goal continuation skipped: the turn stopped without finishing"
            );
            return Ok(());
        }

        // Bounded safety: don't run the continuation loop forever even
        // if the runtime returns Continue on every tick. Normal stagnation
        // exits well before this via replan/AwaitingUser; the cap is only an
        // emergency invariant.
        const MAX_CONTINUATIONS: usize = 32;

        let initial_goal_id: Option<String> =
            match goal_store.active_for_session(&self.session_id).await {
                Ok(Some(goal)) => Some(goal.id.clone()),
                Ok(None) => return Ok(()),
                Err(e) => {
                    tracing::warn!("goal continuation initial load failed: {e}");
                    return Ok(());
                }
            };
        let mut previous_evidence: Option<codegg_core::goal::progress::GoalContinuationEvidence> =
            None;
        let mut consecutive_no_progress: u8 = 0;

        for _ in 0..MAX_CONTINUATIONS {
            // A continuation prompt that could not be queued marks the turn
            // as not-processed; another iteration would only repeat the drop.
            if let Some(reason) = self.retained_stop_reason.clone() {
                tracing::info!(
                    session_id = %self.session_id,
                    reason = %reason,
                    "goal continuation stopped after an unqueued prompt"
                );
                return Ok(());
            }
            // User steering/cancellation interrupts continuation normally and
            // is never converted into blocker progression.
            if self.steering.load(Ordering::SeqCst)
                || self.cancel_rx.as_ref().is_some_and(|rx| *rx.borrow())
            {
                tracing::info!(
                    session_id = %self.session_id,
                    "goal continuation interrupted by user; stopping"
                );
                return Ok(());
            }

            let goal = match goal_store.active_for_session(&self.session_id).await {
                Ok(Some(goal)) => goal,
                Ok(None) => return Ok(()),
                Err(e) => {
                    tracing::warn!("goal runtime decision failed: {e}");
                    return Ok(());
                }
            };
            // Replacement aborts the stale continuation decision.
            if let Some(ref initial) = initial_goal_id {
                if goal.id != *initial {
                    tracing::info!(
                        session_id = %self.session_id,
                        "goal replaced during continuation; aborting stale path"
                    );
                    return Ok(());
                }
            }
            // Budget/terminal gate remains authoritative. Progress policy never
            // revives Paused/Cancelled/BudgetLimited/Complete goals.
            let budget_decision = crate::goal::runtime::should_continue(&goal);
            if !budget_decision.should_continue {
                if let Some(prompt) = budget_decision.prompt {
                    // Final wrap-up prompt (e.g. budget-limited).
                    if let Err(error) = self.follow_up_tx.try_send(prompt) {
                        self.note_follow_up_drop("goal wrap-up prompt", &error);
                    }
                    self.drain_follow_up(request, all_events, processor).await?;
                } else {
                    tracing::info!(
                        session_id = %self.session_id,
                        goal_id = %goal.id,
                        reason = %budget_decision.reason,
                        "goal continuation stopped (terminal status)"
                    );
                }
                return Ok(());
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
                        self.note_follow_up_drop("goal continuation prompt", &error);
                    }
                    self.drain_follow_up(request, all_events, processor).await?;
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
                        self.note_follow_up_drop("goal wait prompt", &error);
                    }
                    self.drain_follow_up(request, all_events, processor).await?;
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
                                self.note_follow_up_drop("goal nudge prompt", &error);
                            }
                            self.drain_follow_up(request, all_events, processor).await?;
                            self.account_goal_for_turn().await;
                        }
                        codegg_core::goal::progress::GoalStagnationStep::ContinueWithReplan => {
                            let prompt = crate::goal::runtime::build_replan_prompt(
                                &goal,
                                reason_code,
                                consecutive_no_progress,
                            );
                            if let Err(error) = self.follow_up_tx.try_send(prompt) {
                                self.note_follow_up_drop("goal replan prompt", &error);
                            }
                            self.drain_follow_up(request, all_events, processor).await?;
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
                            return Ok(());
                        }
                    }
                }
            }
        }
        tracing::warn!("goal continuation hit MAX_CONTINUATIONS={MAX_CONTINUATIONS}, halting");
        Ok(())
    }

    /// Record the reason this run stopped when it is not the provider's own
    /// finish reason.
    ///
    /// First writer wins: the cause that stopped the loop is the cause that is
    /// published, and a later bookkeeping stop (a follow-up drain that finds
    /// the budget already spent) must not rewrite it.
    pub(super) fn record_stop_reason(&mut self, reason: String) {
        if self.retained_stop_reason.is_none() {
            self.retained_stop_reason = Some(reason);
        }
    }

    /// The stop reason this run publishes instead of the provider's finish.
    ///
    /// An interrupt or cancel is reported as `interrupted` regardless of the
    /// retained limit string, so the user's own control is never relabeled as
    /// a budget stop. `None` means the provider finish (or `completed`) still
    /// describes the turn.
    pub(super) fn turn_stop_reason(&self) -> Option<String> {
        if self.steering.load(Ordering::SeqCst)
            || self.cancel_rx.as_ref().is_some_and(|rx| *rx.borrow())
        {
            return Some("interrupted".to_string());
        }
        self.retained_stop_reason.clone()
    }

    /// Make an unqueued continuation prompt part of the turn result.
    ///
    /// A warn log is not a record a consumer can see: the prompt was never
    /// queued, so the work it described did not happen. Retaining a reason
    /// keeps the turn from publishing a plain `completed`, and the reason
    /// names the drop. Channel capacity is not raised.
    fn note_follow_up_drop(
        &mut self,
        what: &str,
        error: &tokio::sync::mpsc::error::TrySendError<String>,
    ) {
        tracing::error!(
            ?error,
            what,
            "follow-up prompt was not queued; reporting the turn as not processed"
        );
        self.record_stop_reason(format!("{what} dropped: follow-up channel unavailable"));
    }

    /// Stop accepting new follow-up prompts into this turn's channel.
    ///
    /// The live-turn registration is what routes a follow-up into this
    /// loop's channel. Removing it before the final drain means a prompt that
    /// arrives after that drain is rejected at the delivery boundary instead
    /// of being accepted into a channel whose receiver is about to be dropped.
    /// The registration's own unregister after `run()` returns stays as the
    /// idempotent backstop.
    pub(super) async fn close_follow_up_intake(&self) {
        let (Some(control), Some(turn_id)) = (&self.services.run_control, self.turn_id.as_ref())
        else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }
        let owner = crate::agent::run_control::LiveTurnOwner {
            session_id: self.session_id.clone(),
            turn_id: turn_id.clone(),
        };
        control
            .unregister_live_turn(&owner, &self.follow_up_tx)
            .await;
    }

    /// Spend one tool-stripped provider call so a budget stop can summarize.
    ///
    /// This is the shape the `agent.steps` cap already uses: a control
    /// instruction, an assistant stub, and no tools. It is called at most once
    /// per run, from the loop break, and never dispatches a tool batch or
    /// re-enters the summary.
    pub(super) async fn summarize_budget_stop(
        &mut self,
        request: &mut ChatRequest,
        all_events: &mut Vec<ChatEvent>,
        processor: &mut EventProcessor,
        model_profile: &crate::model_profile::types::ResolvedModelProfile,
        reason: &str,
    ) -> Result<(), AppError> {
        let system = format!(
            "CRITICAL - EXECUTION BUDGET REACHED\n\nThis turn stopped because {reason}. Tools are no longer available. Provide a short summary of what you completed and what remains, then exit."
        );
        crate::model_profile::policy::push_control_instruction(
            &mut request.messages,
            model_profile,
            &system,
        );
        request.messages.push(Message::Assistant {
            content: vec![ContentPart::Text {
                text: "Here is a summary of my work so far:".to_string().into(),
            }],
            tool_calls: vec![],
        });
        let tools = request.tools.take();
        self.lifecycle
            .set_phase(crate::agent::coordinator::TurnPhase::ProviderInvocation);
        let turn_retry = crate::provider::RetryContext::for_operation();
        let received =
            crate::agent::provider_turn::ProviderTurnAdapter::receive_with_retry_context(
                self,
                request,
                Some(turn_retry),
            )
            .await;
        request.tools = tools;
        let events = received?;
        for event in &events {
            processor.process(event);
        }
        all_events.extend(events);
        if let Some(message) = processor.to_assistant_message() {
            request.messages.push(message);
        }
        processor.reset();
        Ok(())
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

    /// Persist this run's conversation into the session transcript.
    ///
    /// Without this, a run leaves nothing behind: the client held the
    /// conversation in memory, so reloading the session replaced the
    /// transcript with whatever imports had written — usually nothing. Called
    /// once per run, after goal continuation has folded any autonomous
    /// follow-ups into `all_events`, so the stored reply is the whole turn.
    ///
    /// Every failure is logged and swallowed. The run has already succeeded;
    /// a transcript write is bookkeeping, and failing it here would turn a
    /// completed agent turn into an error the user has to reason about.
    pub(super) async fn persist_turn_transcript(&self, events: &[ChatEvent]) {
        let Some(store) = self.services.message_store.clone() else {
            return;
        };
        if self.session_id.is_empty() {
            return;
        }
        let Some(prompt) = self.current_user_prompt.as_deref() else {
            return;
        };

        let prefix = format!("msg_{}", uuid::Uuid::new_v4().simple());
        let transcript =
            super::transcript::build_turn_transcript(&self.session_id, &prefix, prompt, events);
        if transcript.entries.is_empty() {
            return;
        }

        let mut batch = Vec::with_capacity(transcript.entries.len());
        for entry in transcript.entries {
            match serde_json::to_value(&entry.data) {
                Ok(value) => batch.push((entry.id, value)),
                Err(error) => {
                    tracing::warn!(
                        session_id = %self.session_id,
                        %error,
                        "session transcript entry could not be encoded; skipping persist"
                    );
                    return;
                }
            }
        }

        match store.append_batch(&self.session_id, &batch).await {
            Ok(Some(written)) => {
                tracing::debug!(
                    session_id = %self.session_id,
                    written,
                    requested = batch.len(),
                    "session transcript persisted"
                );
            }
            // Synthetic transcripts (subagents, continuation tasks) have no
            // durable session row, so this write is correctly skipped. A
            // delegated run's conversation is durable elsewhere: ADR-0051's
            // delegated transcript store, keyed by the run id, written by the
            // child's own capture path. No `session` row is created for it.
            Ok(None) => {
                tracing::debug!(
                    session_id = %self.session_id,
                    "session transcript skipped: no durable session"
                );
            }
            Err(error) => {
                tracing::warn!(
                    session_id = %self.session_id,
                    %error,
                    "session transcript persist failed"
                );
            }
        }
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
    use super::*;
    use crate::agent::processor::EventProcessor;
    use crate::agent::r#loop::{AgentLoop, AgentLoopState};
    use crate::bus::events::AppEvent;
    use crate::provider::{ChatEvent, ChatRequest, ContentPart, Message, Provider, ProviderError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    /// One scripted provider response: the events a single `stream` call emits.
    type ScriptedResponse = Vec<Result<ChatEvent, ProviderError>>;

    #[derive(Clone)]
    struct RecordingProvider {
        calls: Arc<AtomicUsize>,
        tools_seen: Arc<Mutex<Vec<bool>>>,
        script: Arc<Mutex<std::collections::VecDeque<ScriptedResponse>>>,
    }

    #[async_trait::async_trait]
    impl Provider for RecordingProvider {
        fn id(&self) -> &str {
            "turn-completion-recording"
        }

        fn name(&self) -> &str {
            "turn-completion-recording"
        }

        fn clone_box(&self) -> Box<dyn Provider> {
            Box::new(self.clone())
        }

        async fn stream(
            &self,
            request: &ChatRequest,
        ) -> Result<crate::provider::EventStream, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.tools_seen
                .lock()
                .expect("tools_seen mutex")
                .push(request.tools.is_some());
            let events = self
                .script
                .lock()
                .expect("script mutex")
                .pop_front()
                .unwrap_or_default();
            Ok(Box::pin(futures_util::stream::iter(events)))
        }

        async fn models(&self) -> Result<Vec<crate::provider::ModelInfo>, ProviderError> {
            Ok(Vec::new())
        }
    }

    fn recording_provider(
        script: Vec<Vec<Result<ChatEvent, ProviderError>>>,
    ) -> (RecordingProvider, Arc<AtomicUsize>, Arc<Mutex<Vec<bool>>>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let tools_seen = Arc::new(Mutex::new(Vec::new()));
        let provider = RecordingProvider {
            calls: calls.clone(),
            tools_seen: tools_seen.clone(),
            script: Arc::new(Mutex::new(script.into())),
        };
        (provider, calls, tools_seen)
    }

    fn finish(stop_reason: &str, input_tokens: usize, output_tokens: usize) -> ChatEvent {
        ChatEvent::Finish {
            stop_reason: Arc::new(stop_reason.to_string()),
            usage: crate::provider::TokenUsage {
                input_tokens,
                output_tokens,
                ..Default::default()
            },
        }
    }

    fn test_loop(provider: Box<dyn Provider>) -> (AgentLoop, tempfile::TempDir) {
        let workspace = tempfile::tempdir().expect("temporary workspace");
        let session_id = format!("turn-completion-{}", uuid::Uuid::new_v4());
        let agent_loop = AgentLoop::new(
            Vec::new(),
            provider,
            crate::permission::PermissionChecker::new(None, None),
            crate::tool::ToolRegistry::default(),
            crate::config::schema::Config::default(),
            None,
            None,
            Arc::new(crate::context::InMemoryArtifactStore::new()),
            workspace.path().to_path_buf(),
            session_id,
            None,
            None,
        );
        (agent_loop, workspace)
    }

    fn test_request(tools: Option<Vec<crate::provider::ToolDefinition>>) -> ChatRequest {
        ChatRequest {
            messages: vec![Message::User {
                content: vec![ContentPart::Text {
                    text: "test prompt".to_string().into(),
                }],
            }],
            model: "test/model".to_string(),
            tools,
            system: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            response_format: None,
            thinking_budget: None,
            reasoning_effort: None,
            context: Default::default(),
        }
    }

    async fn receive_finished(
        receiver: &mut tokio::sync::broadcast::Receiver<AppEvent>,
        expected_session: &str,
    ) -> String {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match receiver.recv().await {
                    Ok(AppEvent::AgentFinished {
                        session_id,
                        stop_reason,
                        ..
                    }) if session_id == expected_session => return stop_reason,
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        panic!("global event bus closed before AgentFinished")
                    }
                }
            }
        })
        .await
        .expect("AgentFinished is published before timeout")
    }

    #[test]
    fn accounting_deltas_are_distinct_from_cumulative_limits() {
        let mut state = AgentLoopState {
            current_agent: "standard".to_string(),
            reasoning_override: None,
            turn_count: 3,
            total_tokens: 100,
            start_time: Instant::now(),
            plan_mode: false,
            plan_topic: None,
            tool_call_count: 5,
            unaccounted_tool_calls: 2,
            unaccounted_input_tokens: 11,
            unaccounted_output_tokens: 7,
            run_provider_responses: 0,
            run_input_tokens: 0,
            run_output_tokens: 0,
            run_cached_tokens: None,
            run_reasoning_tokens: None,
            run_provider_allowance_micros: None,
            executed_tool_outcomes: Default::default(),
        };

        // A successful accounting tick consumes only the delta; the hard
        // limit counter remains cumulative for subsequent checks.
        state.unaccounted_tool_calls = 0;
        state.unaccounted_input_tokens = 0;
        state.unaccounted_output_tokens = 0;
        assert_eq!(state.tool_call_count, 5);
        assert_eq!(state.unaccounted_tool_calls, 0);
    }

    #[test]
    fn turn_usage_prefers_the_retained_limit_reason() {
        let events = vec![finish("tool_calls", 120, 35)];
        let retained = turn_usage(&events, Some("max turns (100) reached"));
        assert_eq!(retained.0, "max turns (100) reached");
        assert_eq!(retained.1, Some(120));
        assert_eq!(retained.2, Some(35));
        assert_eq!(turn_usage(&events, None).0, "tool_calls");
    }

    #[tokio::test]
    async fn publish_agent_finished_reports_the_retained_reason() {
        let (provider, _, _) = recording_provider(Vec::new());
        let (mut agent_loop, _workspace) = test_loop(Box::new(provider));
        let session_id = agent_loop.session_id.clone();
        let mut receiver = crate::bus::global::GlobalEventBus::subscribe();
        let events = vec![finish("tool_calls", 120, 35)];

        agent_loop.record_stop_reason("max turns (100) reached".to_string());
        agent_loop.publish_agent_finished(&events);

        assert_eq!(
            receive_finished(&mut receiver, &session_id).await,
            "max turns (100) reached"
        );
    }

    #[tokio::test]
    async fn an_interrupt_outranks_a_retained_limit_reason() {
        let (provider, _, _) = recording_provider(Vec::new());
        let (mut agent_loop, _workspace) = test_loop(Box::new(provider));
        let session_id = agent_loop.session_id.clone();
        let mut receiver = crate::bus::global::GlobalEventBus::subscribe();

        agent_loop.record_stop_reason("max turns (100) reached".to_string());
        agent_loop.steering.store(true, Ordering::SeqCst);
        agent_loop.publish_agent_finished(&[finish("tool_calls", 120, 35)]);

        assert_eq!(
            receive_finished(&mut receiver, &session_id).await,
            "interrupted"
        );
    }

    #[tokio::test]
    async fn the_summary_handoff_removes_tools_and_runs_once() {
        let (provider, calls, tools_seen) =
            recording_provider(vec![vec![Ok(finish("stop", 12, 4))]]);
        let (mut agent_loop, _workspace) = test_loop(Box::new(provider));
        let mut request = test_request(Some(vec![crate::provider::ToolDefinition {
            name: "read".to_string(),
            description: "read files".to_string(),
            parameters: serde_json::json!({"type":"object"}),
            defer_loading: None,
        }]));
        let mut all_events = Vec::new();
        let mut processor = EventProcessor::new();
        let config = crate::config::schema::Config::default();
        let profile =
            crate::model_profile::ModelProfileResolver::new(&config).resolve(&request.model);

        agent_loop
            .summarize_budget_stop(
                &mut request,
                &mut all_events,
                &mut processor,
                &profile,
                "max turns (1) reached",
            )
            .await
            .expect("the bounded summary call completes");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            tools_seen.lock().expect("tools_seen mutex").as_slice(),
            &[false]
        );
        assert!(
            request.tools.is_some(),
            "the original tool surface is restored"
        );
        assert!(all_events
            .iter()
            .any(|event| matches!(event, ChatEvent::Finish { .. })));
    }

    #[tokio::test]
    async fn a_normal_finish_still_completes() {
        let (provider, _, _) = recording_provider(Vec::new());
        let (mut agent_loop, _workspace) = test_loop(Box::new(provider));
        let session_id = agent_loop.session_id.clone();
        let mut receiver = crate::bus::global::GlobalEventBus::subscribe();
        let events = vec![finish("stop", 3, 2)];

        assert_eq!(turn_usage(&events, None).0, "stop");
        agent_loop.publish_agent_finished(&events);
        assert_eq!(receive_finished(&mut receiver, &session_id).await, "stop");

        assert_eq!(turn_usage(&[], None).0, "completed");
        agent_loop.publish_agent_finished(&[]);
        assert_eq!(
            receive_finished(&mut receiver, &session_id).await,
            "completed"
        );
    }
}
