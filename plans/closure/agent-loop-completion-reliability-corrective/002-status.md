# Agent Loop Completion Reliability C002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-completion-reliability-corrective/002-follow-up-drain-cannot-bypass-the-turn.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c002--follow-up-drain-stays-inside-the-turn`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Implementation state: uncommitted working-tree changes in this worktree. This record
does not claim a commit or a verified commit head.

## 1. Executive finding

The follow-up drain is a second provider/tool loop that had drifted from the main
loop. It now consults the same `check_limits` and cancellation state before each of
its provider calls, propagates a tool-executor error instead of returning `Ok(())`,
reports an unqueued continuation prompt on the turn result, and closes live-turn
intake before its final drain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The drain observes the spent budget before a provider call | `the_follow_up_drain_obeys_the_spent_budget` | pass | `max_turns = 0`: zero provider calls and `turn_stop_reason() == "max turns (0) reached"`. |
| A drain limit/cancel uses C001's publication | same test plus `turn_stop_reason` | pass | The drain records the reason instead of inventing a parallel vocabulary. |
| A tool-executor error is not `Ok(())` | source: `drain_follow_up` `return Err(e)` | **not exercised by a test** | See §10. |
| An unqueued prompt is visible | `note_follow_up_drop` records a reason; five call sites converted | pass (source) | The reason names the drop; channel capacity stays 32. |
| Intake closes before the final drain | `close_follow_up_intake` call order in `run_inner` | pass (ordering proof) | See §10 for the exact ordering argument. |

## 3. Production implementation evidence

- `drain_follow_up`'s inner loop now begins with `check_limits()` (records and returns
  `Ok(())` on a hit) and the cancel watch, then proceeds to compaction and the
  provider call.
- A tool-execution `Err` returns `Err(e)`, matching the main loop's `?` and the
  existing `TurnFailed` classification.
- `maybe_continue_goal` no longer starts a new provider loop when
  `turn_stop_reason()` is set (C001's contract), and stops the continuation loop after
  an unqueued prompt.
- `AgentLoop::close_follow_up_intake` unregisters the live turn with the same
  `(session_id, turn_id)` pair and the same channel the registration used, immediately
  before the final `drain_follow_up`; `turn_runtime`'s later unregister is an
  idempotent no-op.

## 4. Verification executed

```bash
cargo test -p codegg --lib agent::follow_up               # 5 passed
cargo test -p codegg --lib agent::turn_completion         # 6 passed
cargo test -p codegg --lib agent::                       # 538 passed, 0 failed
python3 scripts/check_scheduler_bypass.py                 # pass
python3 scripts/check_idle_continuation_callsites.py      # pass
scripts/verify.sh quick                                   # passed
```

## 5. Invariant review

- The main loop still uses `?`; the regression test family still drives it.
- `FOLLOW_UP_CHANNEL_CAPACITY` stays 32.
- `MAX_CONTINUATIONS` stays 32.
- No new scheduler, queue, or idle-continuation entry; ADR-0031's caller set is
  unchanged (the idle-continuation guard passes).
- ToolBroker remains the only native tool boundary.

## 6. Failure and recovery review

A cancel during a follow-up tool call still relies on the tool's own cancellation; this
milestone only requires that the next provider iteration not start. No second kill was
added around an in-flight tool. A retained prompt that dies with the process is
acceptable only because the drop is the same visible outcome the unqueued-prompt path
reports; no table was added for one prompt.

## 7. Migration and compatibility review

No schema change. These turns publish `TurnFailed` or a non-`completed` reason instead
of a plain completion; that is the correction.

## 8. Security review

No permission or sandbox change. Drain tool calls still run through the same batch and
broker path as the main loop.

## 9. Documentation and operations

`architecture/agent.md` states that the follow-up drain shares the turn budget and that
a drain tool failure fails the turn.

## 10. Unresolved findings

- **No test forces a batch-level `Err` during the drain.** Every failure inside
  `execute_tool_calls_impl` becomes a `ToolError` outcome, not an `Err`; the function
  ends in `Ok(ordered_results)`, and its only `?` sites are inside tool-level
  closures. Forcing a batch-level `Err` would need a production fault-injection seam,
  which is outside this milestone. The change is a one-line propagation identical to
  the main loop's, and the main-loop behavior is still covered.
- **Intake ordering is proved by code order, not by a race test.** `live_turns` is the
  turn-level endpoint the plan named; the run-level `live` handle is owned by the run
  worker and out of this milestone's boundary. The two remaining readers of
  `live_turns` are best-effort completion pushes whose durable notice row is
  authoritative, so closing intake early costs latency, not a notice.

## 11. Roadmap disposition

C002 is closed on C001's published reason.

## 12. Registry updates

The addendum and the registry row for C002 are set to closed.
