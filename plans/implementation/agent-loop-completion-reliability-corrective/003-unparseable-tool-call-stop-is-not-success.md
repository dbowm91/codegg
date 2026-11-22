# Agent Loop Completion Reliability C003 — An Unparseable Tool-Call Stop Is Not Success

Status: closed
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c003--an-unparseable-tool-call-stop-is-not-success`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`
Applicable ADRs: none new.
Primary class: invariant

## 1. Objective

When the model stops with `stop_reason=tool_calls` and the loop cannot parse a tool call after its existing retries, the turn is not published as a successful completion.

## 2. Why this milestone is ready

The parse-retry path already exists and already emits `AppEvent::Error`. The defect is the terminal classification after that error, not a missing parser. C001 and C002 do not own this break. No protocol change is required if `TurnFailed` can carry the existing error text.

## 3. Current implementation evidence

In the main loop, an empty tool-call list whose provider `stop_reason` is `tool_calls` logs a warning, publishes `AppEvent::Error` with the raw-text preview, resets nothing further, and falls through to the work-plan arbiter and then the normal loop `break` (`src/agent/loop.rs`, the `stop_reason=tool_calls` arm). `DefaultTurnRuntime` then publishes `TurnCompleted` on the `Ok` path.

`drain_follow_up` contains the same warning, the same `AppEvent::Error`, and a `break` out of its inner loop (`src/agent/follow_up.rs`). The outer drain then returns `Ok`, so the same success publication applies.

The message says the failure is "after retries." This milestone must not add another retry. The remaining defect is that the turn still succeeds.

A work-plan `NeedsUserJudgment` or `BlockedReport` also ends the loop without marking the plan complete. That is the closed long-horizon M003 contract. It is not this milestone. Do not change those arms.

## 4. Invariants that must not regress

- The existing parse repair and retry stay. Do not remove them, and do not add a new retry after they are exhausted.
- `AppEvent::Error` may remain as a side publication. It must not be the only signal.
- The work-plan arbiter's "end the turn without completing the plan" arms stay. Do not relabel `NeedsUserJudgment` or `BlockedReport` as `TurnFailed`.
- A genuine provider finish with no tool calls, and a soft-stop continuation that `continuation_allowed` still accepts, still complete or continue as they do today.
- No new tool parser and no second tool-call vocabulary.

## 5. Scope

In:

- Both copies of the exhausted `stop_reason=tool_calls` arm return an error the runtime publishes as `TurnFailed`, or they set a retained terminal reason that cannot be overwritten by `completed`.
- The published message keeps the existing bounded preview. It does not grow the 600-character preview.
- A test covers the main loop and the follow-up copy. Fixing one copy is not closure.

Out:

- A new parse strategy, a new model retry, or a change to `is_soft_stop_reason`.
- Work-plan judgment and blocked endings.
- Budget publication (C001) and follow-up budget bypass (C002), except that this arm must not `break` into those success paths.
- Frontend wording, unless the daemon publication is impossible without one. Then stop.

## 6. Required production changes

- `src/agent/loop.rs`: the exhausted `tool_calls` arm.
- `src/agent/follow_up.rs`: the matching arm.
- Runtime publication only if an `Err` return is not already `TurnFailed`. Prefer the existing `TurnFailed` path used for provider errors.
- `architecture/agent.md`: one sentence that an exhausted unparseable tool-call stop fails the turn.

## 7. Ordered work packages

### WP-A — Main-loop arm fails the turn

Intent: the warning and `AppEvent::Error` are not followed by `TurnCompleted`.

Required changes: return `Err` with the existing preview text, or an equivalent typed error `classify_turn_failure` already understands. Do not add a variant if the classifier has a generic provider/tool class that fits. If none fits, stop.

Acceptance evidence: a fixture with `stop_reason=tool_calls` and no parseable call publishes `TurnFailed` and does not publish `TurnCompleted`.

### WP-B — Follow-up copy matches

Intent: the drift that C002 is about does not leave a second success path.

Required changes: the same terminal as WP-A. A `break` that returns `Ok` from the drain is not enough.

Acceptance evidence: the same fixture through `drain_follow_up` publishes `TurnFailed`.

### WP-C — Ordinary completion stays ordinary

Intent: the fix does not fail every empty tool list.

Required changes: none if the condition stays `stop_reason == tool_calls` after the existing retries. A soft-stop continuation that is still allowed must still continue.

Acceptance evidence: an empty tool list with a normal stop reason still completes. A soft-stop that `continuation_allowed` accepts still continues once.

## 8. Failure, cancellation, restart, contention semantics

- The failure is terminal for this turn. The next user message can retry. This milestone does not auto-continue.
- Cancel wins if it is already set. Do not overwrite `interrupted` with this parse failure.
- No durable state. A crash before publication stays a crash.
- No lock and no second parser task.

## 9. Compatibility and migration

No schema change. Sessions that previously showed a completed turn plus an error toast will show a failed turn. That is the correction. The preview bound stays 600 characters so the failure event cannot carry the whole raw stream.

## 10. Required tests

- WP-A, WP-B, and WP-C.
- A negative test that `NeedsUserJudgment` is not `TurnFailed` solely because of this change. If that arm is hard to reach in the same harness, cite the arm and do not edit it.
- No live model.

## 11. Required verification commands

```bash
cargo test -p codegg --lib agent::
cargo test --test agent_loop -- agent_loop_harness
```

Use the family form. `cargo test --test agent_loop_harness` selects zero tests.

Implementation closure also runs `scripts/verify.sh quick`. This planning document does not claim that run.

## 12. Documentation updates

One sentence in `architecture/agent.md`. Do not edit the closed long-horizon closure records.

## 13. Acceptance criteria

- A model response that says it is calling tools, and yields no parseable call after the existing retries, ends the turn as a failure the session can see without a log.
- The same response inside a follow-up drain ends the same way.
- A normal answer with no tool calls still completes.
- The work-plan judgment and blocked arms are unchanged.

## 14. Stop conditions

- `TurnFailed` cannot carry this condition without a new category or protocol variant. Stop and report. Do not add the variant here.
- The only way to detect the condition is a new parser. Stop. The existing `stop_reason` match is the condition.
- The work-plan fall-through cannot be skipped without changing an arbiter decision. Return before the arbiter on this arm only. Do not edit the arbiter.

## 15. Closure evidence required

- Both fixtures and their published events.
- Confirmation the work-plan arms were not edited.
- Confirmation no retry was added after the existing exhausted path.
- The architecture sentence.

## 16. Handoff notes

This is a classification bug on a path that already knows it failed. Do not "improve parsing" inside this milestone. Re-read both arms before editing; they have drifted once and will drift again if only one is patched.
