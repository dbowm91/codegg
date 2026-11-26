# Agent Loop Completion Reliability C002 — Follow-up Drain Cannot Bypass the Turn or Hide a Tool Failure

Status: closed
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c002--follow-up-drain-stays-inside-the-turn`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`
Applicable ADRs: none new. ADR-0031 and the closed goal-continuation contract stay in force. This plan does not add a continuation engine.
Primary class: invariant

## 1. Objective

A queued follow-up is either processed inside the same execution budget as the turn that drains it, or it is visibly not processed. A tool-execution failure during that drain is not returned as success.

## 2. Why this milestone is ready

C001 owns the main-loop budget publication. This milestone is independently executable: `drain_follow_up` is a second provider/tool loop, and its failure path does not go through `check_limits` or `TurnFailed`. No open milestone owns it. The closed long-horizon and idle-continuation lines are consumers, not blockers.

## 3. Current implementation evidence

- `drain_follow_up` (`src/agent/follow_up.rs`) drains with `try_recv` and then runs an inner provider/tool loop. A search of that file found no `check_limits` call. `turn_count` is read once and not incremented. Cancellation is not checked in the drain's own loop.
- The main loop checks limits, cancellation, and steering before each provider call (`src/agent/loop.rs`). The follow-up loop does not. A goal can therefore keep making provider and tool calls after the main loop has already stopped for budget.
- On tool-executor `Err`, the follow-up loop logs `Tool execution error`, resets the processor, and `return Ok(())` (`src/agent/follow_up.rs`). The main loop's matching call uses `?` and becomes `TurnFailed`. The follow-up copy does not.
- Goal continuation queues with `try_send`. On failure it logs `goal continuation prompt dropped` and still calls `drain_follow_up` (`src/agent/turn_completion.rs`). A full channel (capacity 32, `src/agent/loop.rs`) drops the prompt. Run-control has a typed `ChannelFull`; this path does not.
- The drain's own comment says a follow-up that arrives after `run()` returns needs a new `run()`. The final drain is non-blocking. A prompt accepted after that drain and before the live-turn sender is unregistered has no later drain in this turn. That race was read from call order, not executed.

## 4. Invariants that must not regress

- The main loop's `?` on tool-batch failure stays a typed `TurnFailed`. Do not make the main loop swallow errors to match the follow-up path.
- `FOLLOW_UP_CHANNEL_CAPACITY` stays 32. Do not raise it as the fix.
- Goal continuation stays bounded by `MAX_CONTINUATIONS`. Do not remove that cap.
- C001's visible budget stop still applies. A follow-up must not restart a capped loop that C001 just stopped.
- No new scheduler, queue, or idle-continuation entry. ADR-0031's caller set is not widened.
- ToolBroker remains the only tool-call boundary.

## 5. Scope

In:

- The follow-up inner loop consults the same `check_limits` and cancellation/steering checks as the main loop before each provider call.
- A limit or cancel hit during drain uses C001's publication. It does not return `Ok` and fall through to `TurnCompleted` with a provider finish reason.
- Tool-executor `Err` during drain is `Err`, or a typed terminal that `DefaultTurnRuntime` publishes as `TurnFailed`. It is not `Ok(())`.
- `try_send` failure of a goal continuation or wrap-up prompt is visible on the turn result. A warn log is not the only record.
- The post-drain / pre-unregister window either retains an accepted prompt for the next turn or the sender is unregistered before the last drain returns. Confirm the order before editing. Do not add a second drain loop to paper over it.

Out:

- Raising channel capacity, `max_turns`, or `MAX_CONTINUATIONS`.
- Redesigning Goal, WorkPlan, or idle continuation.
- The unparseable `stop_reason=tool_calls` classification. That is C003, including the copy inside this drain.
- Provider retry policy.
- A new protocol variant.

## 6. Required production changes

- `src/agent/follow_up.rs`: limit, cancel, and tool-error propagation.
- `src/agent/turn_completion.rs`: the `try_send` failure path, only so a dropped goal prompt is not warn-only.
- `src/agent/turn_runtime.rs` or `loop.rs` only if the sender stays registered across the final drain. The change is ordering or retention, not a new channel.
- Docs: one paragraph in `architecture/agent.md` that the follow-up drain is inside the turn budget and that a drain tool failure fails the turn.

## 7. Ordered work packages

### WP-A — Same budget, same cancel

Intent: the inner loop cannot outlive the turn budget or ignore cancel.

Required changes: before each provider call in the drain, call `check_limits` and the cancel/steering checks. On a hit, stop the drain with the retained reason from C001. Do not increment a second private turn counter. If C001 has not landed, stop and report; do not invent a parallel reason string.

Acceptance evidence: a drain whose `max_turns` is already exhausted performs zero provider calls. A cancel observed at the inner-loop boundary performs zero further tool calls and publishes `interrupted`.

### WP-B — Tool failure is failure

Intent: the follow-up copy of tool execution matches the main loop's `?`.

Required changes: replace `return Ok(())` after `Tool execution error` with an error return that the runtime already maps to `TurnFailed`. Do not log-and-succeed.

Acceptance evidence: a forced tool-executor error during drain publishes `TurnFailed` and does not publish `TurnCompleted`. The same error on the main-loop path is unchanged.

### WP-C — A dropped prompt is visible

Intent: `try_send` failure is part of the turn result.

Required changes: if `try_send` fails, the turn's published stop or error names the drop. Do not continue as if the prompt was queued. Do not enlarge the channel.

Acceptance evidence: a full channel drops the goal prompt and the turn result is not a plain `completed`. A prompt that fits is still drained.

### WP-D — No silent accept-after-drain

Intent: a prompt accepted after the last drain is not destroyed by unregister.

Required changes: read the unregister site in `turn_runtime.rs` and the final `drain_follow_up` call. If a send can succeed in between, retain that prompt for the next `run()` of the same session or unregister first. If the code already makes that window impossible, add the test that shows it and do not add retention.

Acceptance evidence: a test or a cited ordering proof. Do not claim a race the test did not exercise.

## 8. Failure, cancellation, restart, contention semantics

- Cancel during a follow-up tool call uses the tool's own cancellation. This milestone only requires the next provider iteration not to start. Do not add a second kill around an in-flight tool.
- A `TurnFailed` from WP-B is the same category path as a main-loop tool failure. Do not invent a follow-up-specific category unless the existing classifier cannot describe it. If it cannot, stop and report.
- Restart: an undrained prompt that was retained must be durable or explicitly dropped with a visible reason. An in-memory retain that dies with the process is acceptable only if the drop is the same visible outcome as WP-C. Do not add a table for one prompt.
- Two drains must not run. The existing single-loop ownership stays.

## 9. Compatibility and migration

No schema change. Clients that only understand `completed` still receive `TurnFailed` or a non-`completed` `stop_reason` for these cases. That is the correction. Do not keep publishing `TurnCompleted` for compatibility.

## 10. Required tests

- WP-A through WP-D as specified.
- A regression that the main loop still uses `?` and still checks limits. C002 must not be fixable by deleting the main-loop checks.
- No live-provider test.

## 11. Required verification commands

```bash
cargo test -p codegg --lib agent::follow_up
cargo test -p codegg --lib agent::turn_completion
cargo test -p codegg --lib agent::
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_idle_continuation_callsites.py
```

The idle-continuation guard is not the owner of this drain, but run it if the edit touches a continuation entry. A failure that is pre-existing and unrelated must be recorded, not silenced.

Implementation closure also runs `scripts/verify.sh quick`. This planning document does not claim that run.

## 12. Documentation updates

`architecture/agent.md` execution lifecycle: follow-up drain is inside `check_limits`, and a tool failure there fails the turn. Do not document a larger channel.

## 13. Acceptance criteria

- After the main loop hits its budget, a queued follow-up does not start another provider or tool call.
- A tool-executor error during drain is a failed turn, visible without reading a log.
- A goal prompt that does not fit in the channel is visible as not queued. The turn does not look successful solely because the log contains a warning.
- A prompt that arrives in time is still processed.
- No new queue, store, or continuation service exists.

## 14. Stop conditions

- C001's retained reason is not available and the drain would need its own stop vocabulary. Stop. Do not fork the publication.
- The tool-error `Ok(())` is load-bearing for a test that expects follow-up tool failure to complete the turn. Fix the test. Do not keep the success return.
- Retention of a late prompt requires a new table or protocol field. Stop and report. Visible drop is the allowed fallback.
- Any widening of ADR-0031's idle-continuation caller set. That is a recorded decision, not this milestone.

## 15. Closure evidence required

- The tests in §10 and §11, with results.
- Quoted assertion that drain tool failure does not publish `TurnCompleted`.
- Quoted assertion or ordering proof for the post-drain window.
- Confirmation that channel capacity and `MAX_CONTINUATIONS` were not raised.
- The architecture paragraph.

## 16. Handoff notes

The main loop and the follow-up loop drifted. The corrective is to make the copy obey the original, not to give follow-ups a private budget. Re-read `drain_follow_up` and the `try_send` site before editing. Line numbers in §3 are evidence locations, not an edit script.
