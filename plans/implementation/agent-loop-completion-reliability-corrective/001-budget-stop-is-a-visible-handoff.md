# Agent Loop Completion Reliability C001 — Budget Stop Is a Visible Handoff

Status: closed
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c001--budget-stop-is-a-visible-handoff`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`
Applicable ADRs: none new. ADR-0040 stays accepted and is not amended.
Primary class: invariant

## 1. Objective

When `AgentLoop::check_limits` stops a turn, the session and the next turn see that budget reason. The stop is not published as a normal completion, and the token cap reads a counter that actually moves.

## 2. Why this milestone is ready

The operational-reliability, recovery-escalation, execution-reliability, and long-horizon workstreams are closed. No open milestone owns this publication path. The fix fits the existing `stop_reason` string and the existing `agent.steps` summary injection. It does not need a new protocol, store, or continuation engine.

## 3. Current implementation evidence

Read in this worktree at the baseline above. Line numbers are the read locations; an implementation agent must re-read before editing.

- `ExecutionLimits::default` is `max_turns: 100`, `max_tokens: 1_000_000`, `timeout: 600s` (`src/agent/loop.rs`, `impl Default for ExecutionLimits`).
- `AgentLoopState::total_tokens` is initialized to 0. A search of `src/agent` for `total_tokens =` found no assignment. The cap check is the only read (`src/agent/turn_completion.rs`, `check_limits`).
- `check_limits` returns reasons for agent steps, max turns, optional max tool calls, max tokens, timeout, and the steering flag. The run loop logs the reason and `break`s (`src/agent/loop.rs`, the `Agent loop stopping` site). The reason is not retained.
- `turn_count` increments once per provider iteration, after the limit check and before the provider call. A sequential tool trajectory stops at 100 provider rounds.
- `turn_usage` publishes the last `ChatEvent::Finish` reason, or the literal `completed` (`src/agent/turn_completion.rs`). `publish_agent_finished` does worse for cancellation: a prior `Finish` is preferred over the steering and cancel flags, and `interrupted` is only used when there is no `Finish` (`src/agent/turn_completion.rs`, the `last_finish` branch before the steering check). A cancel or budget break after any provider finish therefore publishes that finish reason. `DefaultTurnRuntime` publishes `CoreEvent::TurnCompleted` from `turn_usage` on the `Ok` path (`src/agent/turn_runtime.rs`).
- `agent.steps` injects a tool-stripped summary prompt on the iteration that would hit the cap. Built-in agents leave `steps: None`, so a normal turn never takes that path. The `ExecutionLimits` break happens before that injection.
- `architecture/agent.md` "Tool-call timeouts" says the 600-second value is not a total turn deadline and must not become one, because a hard cut-off kills long builds. The code checks it at the next iteration boundary, not around the tool-batch await. Keep that.
- `maybe_continue_goal` runs after the loop for a session with an active goal (`src/agent/loop.rs` call; implementation in `src/agent/turn_completion.rs`, `MAX_CONTINUATIONS = 32`). It does not receive the discarded limit reason.
- `host/turn.rs` maps the strings `max_turns` / `max_steps` / `max_tool_calls` / `tool_calls` to `StopReason::MaxTurns`. That mapper is not fed by `check_limits`.

## 4. Invariants that must not regress

- ADR-0040 Option B: unknown progress is not a terminal stall. Do not add `RecoveryDecision::Stalled`.
- A new successful read is not a repeat. Equivalent-error identity stays the whole `action_key`. Do not edit `src/agent/progress_recovery.rs` except to keep its comment that the outer bound is `max_turns` accurate.
- Provider retry stays bounded and does not replay after visible output.
- An in-flight tool batch is not cancelled by `ExecutionLimits.timeout`.
- User interrupt and cancel still publish `interrupted`, not a budget reason.
- ToolBroker remains the only production tool-call boundary. This milestone does not dispatch tools.
- No `std::env::current_dir` in the turn path. No new writer, no scheduler bypass.

## 5. Scope

In:

- Retain the `check_limits` reason across the loop break.
- Publish that reason on `CoreEvent::TurnCompleted.stop_reason` and `AppEvent::AgentFinished.stop_reason` when it is the cause of the stop.
- Give `max_turns`, iteration-boundary `timeout`, and `max_tool_calls` the same one-shot, tool-stripped summary handoff `agent.steps` already uses, or publish the stop with no extra call if that handoff cannot be reused without recursing.
- Make `total_tokens` follow provider usage already stored on `AgentLoopState` (`run_input_tokens` / `run_output_tokens`, or the unaccounted deltas if those are the only values present at the check). When the cap fires, use the same visible stop.
- Prove goal continuation does not start another identical capped loop after this stop.
- Correct the architecture note so the iteration-boundary check, the visible stop, and the no-mid-tool-kill rule agree.

Out:

- Raising 100, 1,000,000, or 600 as the remedy.
- A mid-tool or mid-provider wall-clock kill.
- A new `CoreEvent` variant, a new table, or a new continuation service.
- Idle-continuation, queue-delivery, or Goal budget redesign.
- Tool-batch timeouts, permission waits, compaction, and delegated-run resume.
- Reopening closed operational-reliability M001–M007.

## 6. Required production changes

- Core / runtime: `src/agent/loop.rs` and `src/agent/turn_completion.rs`. The publication site in `src/agent/turn_runtime.rs` only if `turn_usage` cannot see the retained reason. Prefer returning the reason through the existing `Ok(events)` path or through loop state that `publish_agent_finished` already reads. Do not publish the reason from a spawned hook.
- Protocol: none if `stop_reason` is a free string at the construction site. Confirm before editing.
- Storage: none.
- Frontend: none. The TUI already renders a latched turn failure from daemon events; this milestone must not require a new TUI widget. If the existing completion rendering cannot show a non-`completed` `stop_reason`, record that as a follow-up rather than building a widget here. The daemon publication is the acceptance gate.
- Notification: `src/core/daemon_bootstrap.rs` announces `NotificationKind::TurnCompleted` at low priority unless `stop_reason == "error"`. The spoken message already interpolates `stop_reason`, which is not enough: a consumer that branches on kind still treats a budget stop as an ordinary completion. Teach that one consumer. Do not add a `NotificationKind` variant. `AwaitingInput`, `Error`, or `TurnFailed` may be reused only if the message still names the budget reason and a provider `error` remains distinguishable. If none of the existing kinds can say both, stop.
- Docs: `architecture/agent.md` tool-timeout note and the execution-lifecycle sentence that describes how a turn ends.

## 7. Ordered work packages

### WP-A — Retain the reason

Intent: the string `check_limits` returns is still available after the `break`.

Required changes: store it on the loop before breaking. Do not log-and-drop. Steering and cancel keep their existing `interrupted` classification even if `check_limits` also returned `interrupted by user`.

Acceptance evidence: a unit or harness test reads the retained reason after a max-turns break. The previous provider `Finish` reason is still in the event list and is not the published reason.

### WP-B — Publish it

Intent: `TurnCompleted` and `AgentFinished` carry the budget reason.

Required changes: `turn_usage` / `publish_agent_finished` prefer the retained limit reason over the last `Finish` and over the `completed` fallback. Interrupt and cancel win over the limit reason.

Acceptance evidence: max-turns, timeout, and max-tokens stops publish a reason that contains the limit identity (turns, timeout, tokens, or tool calls) and is not `completed`. A cancel or steering stop that already has a provider `Finish` in the event list still publishes `interrupted`, not that finish reason. A normal finish with no limit hit and no cancel still publishes the provider finish reason. The daemon notification for a budget stop is not `NotificationKind::TurnCompleted`. A provider `stop_reason` of `error` is still the failure announcement. The projection may keep `TurnStatus::Completed` and store the reason; do not add a status variant.

### WP-C — One-shot handoff, or none

Intent: the model is not stopped mid-tool-use with no statement that the budget ended, unless a summary call would recurse or cross cancel/timeout.

Required changes: reuse the `agent.steps` injection (termination instruction, tools removed, one provider call) for an `ExecutionLimits` stop that is not an interrupt and is not a timeout that has already elapsed. If reuse would increment `turn_count` in a way that immediately breaks again, or would leave tools available, skip the call and publish the stop. Do not add a retry loop around the summary.

Acceptance evidence: a test where `max_turns` is 1 or 2 shows at most one summary provider call, zero tool dispatches after the stop decision, and a published budget reason. A timeout that is already elapsed performs zero extra provider calls.

### WP-D — Live token counter

Intent: the 1,000,000 cap can fire, and when it fires it uses WP-B.

Required changes: update `state.total_tokens` from usage the run already recorded. Do not parse provider payloads a second time. Do not count a usage report twice. If the only honest statement is that no usage has been observed, leave the counter unchanged and do not pretend the cap was enforced.

Acceptance evidence: a fixture that records input and output usage advances `total_tokens` by that sum and, when the sum crosses a test cap, publishes the token reason. A fixture with no usage report does not trip the cap.

### WP-E — Goal continuation must not relaunch the same cap

Intent: `maybe_continue_goal` does not treat a loop-budget stop as a finished model turn and start another 100-iteration loop.

Required changes: pass the retained reason into the continuation decision, or have the continuation read it. A loop-budget stop returns without a new provider loop. Do not change Goal's own `should_continue` vocabulary. If the existing goal-budget refusal already covers this once the reason is visible, add the test and do not edit the goal runtime.

Acceptance evidence: a session with an active continuable goal hits `max_turns` and does not enter a second provider loop. A goal that is already `BudgetLimited` still does not resume. A normal unfinished goal with no loop-budget stop still continues, up to the existing `MAX_CONTINUATIONS` cap.

## 8. Failure, cancellation, restart, contention semantics

- Cancel or steering during the summary call publishes `interrupted` and does not wait out the budget.
- A summary provider error publishes the existing `TurnFailed` path. It must not be rewritten as a successful budget stop, and it must not be retried into tool execution.
- Daemon restart mid-turn keeps today's recovery policy. This milestone does not add a resume cursor. The published reason is for the turn that completed its publication. A crash before publication stays a crash, not a fake budget stop.
- No new lock. The reason is loop-local and per turn, which matches the existing per-turn `AgentLoop`.

## 9. Compatibility and migration

No stored schema change. Old clients that ignore an unrecognized `stop_reason` still receive `TurnCompleted`; they must not be required to understand the new strings for the daemon fix to be true. Do not change the `completed` string used by a real provider finish. A missing `Finish` plus a retained limit reason is the only new publication.

`ExecutionLimits` defaults stay 100 / 1,000,000 / 600. Config that sets `agent.steps` keeps the steps injection. Both may apply; the first stop wins, and the published reason names which one.

## 10. Required tests

- Focused: WP-A through WP-E examples above, in the existing agent-loop test family. Do not add a new integration executable for one assertion.
- The max-turns test must fail on the baseline behavior: published reason is `completed` or the previous `Finish` reason, and `total_tokens` stays 0 after recorded usage.
- One assertion that a budget `stop_reason` is not announced as `NotificationKind::TurnCompleted`. A provider `error` reason still is the failure announcement.
- Restart: not required. No durable migration.
- Contention: not required. Single-loop state.
- Security: a negative test that the summary handoff has `request.tools = None` and that no tool executor is invoked after the stop decision.
- Do not weaken `progress_recovery` regression tests.

## 11. Required verification commands

```bash
cargo test -p codegg --lib agent::turn_completion
cargo test -p codegg --lib agent::
cargo test --test agent_loop -- agent_loop_harness
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_scheduler_bypass.py
```

The `agent_loop` family form is required. `cargo test --test agent_loop_harness` selects zero tests and exits 0; do not cite it.

Implementation closure also runs `scripts/verify.sh quick`. This planning document does not claim that run.

## 12. Documentation updates

- `architecture/agent.md`: the limit stop is visible; the 600-second check is an iteration-boundary check, not a mid-tool kill, and not "only after the whole turn completes"; the token cap reads a live counter.
- Do not edit `plans/000-` or ADR-0040.
- The closure record, not this plan, states what landed.

## 13. Acceptance criteria

- A turn that reaches 100 provider iterations ends with a session-visible budget reason, not `completed` and not the previous tool-call finish reason.
- A turn whose iteration-boundary elapsed time crosses 600 seconds ends the same way, and a tool call already in flight is not cancelled by that check.
- A turn whose recorded provider usage crosses `max_tokens` ends the same way. A turn with no usage report is not stopped by the token cap.
- The user can distinguish this stop from a finished assistant reply without reading the daemon log.
- The next user message is not required to guess that the previous turn hit a budget. The reason is in the completion event. If the existing transcript or continuation projection does not carry it, WP-C's summary text or an existing system projection must. Do not add a new projection type to satisfy this sentence; stop and report if none of those surfaces can hold it.
- A continuable goal does not start a second capped loop after the stop.

## 14. Stop conditions

- `CoreEvent::TurnCompleted.stop_reason` is a closed enum that cannot represent the reason. Stop. Write an ADR. Do not add a variant in this milestone.
- The steps injection cannot be reused without leaving tools enabled or without recursing. Publish the stop with no extra provider call. Do not design a third handoff.
- Wiring `total_tokens` would double-count or would require a new usage parser. Stop and report. Do not delete the check to make the milestone close, and do not invent a counter.
- The TUI cannot show the reason and the only fix is a new widget or protocol field. Land the daemon publication, record the frontend gap, and do not expand into the TUI.
- Any temptation to raise the numeric caps, restore `Stalled`, or wrap the tool batch in the 600-second timeout. Those are out of scope. Report them; do not do them.

## 15. Closure evidence required

- The tests in §10 and §11, with results.
- The published reason strings, quoted from the test assertion rather than from a log line.
- Confirmation that `RecoveryDecision` still has no `Stalled` variant and that ADR-0040 was not edited.
- Confirmation that no `CoreEvent` variant and no migration were added.
- An explicit statement of whether goal continuation needed a code change or only a test.
- `architecture/agent.md` diff summary.

## 16. Handoff notes

The defect is the discarded reason, not the number 100. A hundreds-of-tool-calls task will still hit `max_turns` if it needs more than 100 provider rounds. After this milestone it stops visibly and can be continued by a later user turn or by whatever continuation already exists. It does not silently look finished.

Do not widen this plan to tool hangs, silent tool results, or compaction failures. Those audits were not evidence for this handoff. If they return defects, they are new milestones in the same addendum.

Re-read `check_limits`, the loop `break`, and `turn_usage` before editing. The line numbers in §3 are evidence locations, not an edit script.
