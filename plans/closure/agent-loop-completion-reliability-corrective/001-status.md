# Agent Loop Completion Reliability C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-completion-reliability-corrective/001-budget-stop-is-a-visible-handoff.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c001--budget-stop-is-a-visible-handoff`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Implementation state: uncommitted working-tree changes in this worktree. This record
does not claim a commit or a verified commit head.

## 1. Executive finding

A `check_limits` stop is now published as that stop. The reason survives the loop
break, outranks the previous provider finish, and is reported on both
`CoreEvent::TurnCompleted.stop_reason` and `AppEvent::AgentFinished.stop_reason`. The
token cap reads a counter that provider usage moves. A budget stop spends at most one
tool-stripped summary call, and a goal continuation does not relaunch an identical
capped loop.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A max-turns stop publishes a budget reason, not `completed` and not the previous finish | `turn_usage_prefers_the_retained_limit_reason`; `publish_agent_finished_reports_the_retained_reason` | pass | The retained reason wins; usage still comes from the last Finish. |
| An interrupt or cancel still publishes `interrupted` | `an_interrupt_outranks_a_retained_limit_reason` | pass | The steering flag outranks a retained limit reason. |
| The summary handoff removes tools and runs once | `the_summary_handoff_removes_tools_and_runs_once` | pass | One provider call, `tools.is_some() == false` at the provider, original tool surface restored after. |
| The handoff cannot dispatch a tool batch | `repeated_unknown_outcomes_remain_bounded_by_the_hard_turn_limit` | pass | `max_turns = 3` yields exactly 4 requests; the fourth scripted response offers a tool call and no fifth request follows. |
| `total_tokens` moves and the cap fires | `a_token_cap_reads_a_counter_that_moves` | pass | Recorded usage accumulates; the cap reads the moved counter. |
| A normal finish still publishes its own reason | `a_normal_finish_still_completes` | pass | `stop` from a Finish; `completed` with no Finish. |
| The daemon does not call a budget stop an ordinary completion | `a_spent_budget_is_not_announced_as_an_ordinary_completion` | pass | Budget reasons map to `AwaitingInput`/Normal; `error` keeps `TurnFailed`/High. |
| An in-flight tool batch is not wrapped in the 600-second check | `check_limits` call site is the loop head only | pass | No code was added around the batch await. |
| Goal continuation does not relaunch after the stop | `maybe_continue_goal` early return on `turn_stop_reason()` | **not exercised by a test** | See §10. |

## 3. Production implementation evidence

- `AgentLoop` retains the stop reason (`retained_stop_reason`), reset per run, and
  `record_stop_reason` is first-writer-wins.
- `turn_stop_reason()` maps steering/cancel to `interrupted` and otherwise returns the
  retained reason.
- `turn_usage` and `publish_agent_finished` use the retained reason first, the last
  Finish second, and `completed` last.
- `limit_stop_allows_summary` excludes `max steps` (its own injection already ran) and
  `timeout (` (the deadline has already elapsed); `summarize_budget_stop` restores the
  tool surface after the single call.
- `provider_turn` mirrors the run accumulator into `total_tokens`.
- `daemon_bootstrap::completion_notification` is the one consumer that now branches on
  `is_budget_stop_reason`; no `NotificationKind` variant was added.

## 4. Verification executed

```bash
cargo test -p codegg --lib agent::turn_completion          # 6 passed
cargo test -p codegg --lib agent::                         # 538 passed, 0 failed
cargo test -p codegg --lib core::daemon_bootstrap          # 7 passed
cargo test --test agent_loop -- agent_loop_harness         # 76 passed, 1 ignored
cargo clippy -p codegg --lib -- -D warnings                # exit 0
cargo fmt --all -- --check                                 # clean
python3 scripts/check_daemon_cwd_usage.py                  # pass
python3 scripts/check_scheduler_bypass.py                  # pass
scripts/verify.sh quick                                    # passed
```

The harness required `cargo build -p codegg-sandbox-helper` first: the sandbox helper
binary is a test prerequisite that `cargo test --test agent_loop` does not build. One
earlier failure in that family was that missing binary, not this change.

## 5. Invariant review

- ADR-0040 is unmodified; `RecoveryDecision` still has no `Stalled` variant.
- No `CoreEvent` variant, store, or migration was added.
- The 600-second check is still consulted at the provider-iteration boundary only.
- `max_turns` (100), `max_tokens` (1,000,000) and `timeout` (600s) are unchanged.
- An interrupt is never relabeled as a budget stop.
- The reason is loop-local; it is not published from a spawned hook task.

## 6. Failure and recovery review

The summary call runs through the existing provider retry/idle bounds. A summary
provider error propagates as the existing `TurnFailed` path, is not rewritten as a
successful budget stop, and is not retried into tool execution. A crash before
publication stays a crash; no resume cursor was added.

## 7. Migration and compatibility review

No schema change. Clients that do not understand a new `stop_reason` string still
receive `TurnCompleted`; the daemon-side correction does not depend on frontend
knowledge. A missing `Finish` plus a retained reason is the only new publication.

## 8. Security review

No permission, sandbox, or writer-fence change. The summary request carries
`tools = None`, so it cannot dispatch a tool, and the test asserts that state at the
provider boundary.

## 9. Documentation and operations

`architecture/agent.md`: the timeout paragraph now says the check is an
iteration-boundary check (not "only after a turn completes"), that a spent budget is
published with a reason, and that the follow-up drain shares the budget. The
`ExecutionLimits` section documents the published reason and the live token counter.

## 10. Unresolved findings

- **Goal-continuation non-relaunch has no automated test.** The early return is in
  `maybe_continue_goal`, but exercising it requires a migrated goal store with an
  active goal row for the session. No such fixture exists in the agent test modules,
  and building one was out of this milestone's file scope. The behavior is a guard on
  the retained reason; a later turn should add the fixture rather than assume the
  branch is unreachable.
- The token cap counts cumulative input+output tokens per run, including the full
  context re-sent on every provider round. A long trajectory can now reach the
  1,000,000 cap where it previously could not. That is the intended correction (a cap
  that cannot fire is not a safeguard), and it stops visibly.
- The summary call is one extra provider round. It is bounded to one call per run and
  cannot recurse.

## 11. Roadmap disposition

C001 is closed. C002's dependency (the published budget reason) is satisfied.

## 12. Registry updates

The addendum and the registry row for C001 are set to closed.
