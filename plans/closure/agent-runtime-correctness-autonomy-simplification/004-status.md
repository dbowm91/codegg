# Agent Runtime Correctness, Autonomy, and Simplification M004 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/004-turn-identity-accounting-and-lifecycle-correctness.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md#7-ordered-milestones`
Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`
Implementation commit: recorded in the registry and Git history for this closure.

## 1. Executive finding

M004 is strictly closed. Turn-local heuristics now use the latest submitted
user message, session-origin context remains separate, goal accounting consumes
only unaccounted deltas, and `AgentLoop` is the sole owner of `AgentFinished`
publication. Daemon `TurnCompleted`/`TurnFailed` publication remains owned by
`TurnRuntime` and occurs independently once per daemon turn.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Current-turn routing uses latest user input | `AgentLoop::latest_user_prompt` scans user messages in reverse; routing and repository recovery use it | pass |
| Research trigger and hint target current turn | Trigger uses the current prompt and hint insertion targets the latest user message | pass |
| Session-origin goal remains stable | `original_user_prompt` is initialized only once and remains the context-frame/security-review source | pass |
| Hard limits remain cumulative | `tool_call_count` is still incremented and checked cumulatively | pass |
| Goal accounting uses exact deltas | `unaccounted_tool_calls` and provider usage accumulators are cleared only after successful accounting | pass |
| Accounting failures do not reset limits or lose deltas | failed storage accounting retains unaccounted state | pass |
| One `AgentFinished` owner | `AgentLoop` publishes success, interruption, and failure summaries; `TurnRuntime` generic duplicates were removed | pass |
| Daemon lifecycle remains distinct | `TurnRuntime` still publishes one `TurnCompleted` or `TurnFailed` event with the daemon turn ID | pass |
| Terminal fidelity | provider stop reason/usage remains on the loop success event; interruption/error paths are classified explicitly and retain accumulated usage where available | pass |
| No schema/protocol redesign | Only internal state/event publication ownership changed; no storage or wire schema migration | pass |

## 3. Implementation evidence

Production changes are limited to `src/agent/loop.rs`,
`src/agent/turn_runtime.rs`, and the corresponding architecture/test
documentation. The loop now has a public error boundary that emits the one
failure `AgentFinished` event before returning the error. `TurnRuntime` retains
the daemon event and error notification but no longer emits a second generic
agent terminal event.

The prompt ownership split is:

- session-origin: `original_user_prompt`, context-frame `user_goal`, and
  security-review historical task context;
- current-turn: latest user prompt for model routing, research triggering,
  repository-task classification, and recovery decisions;
- provider/accounting: cumulative hard-limit calls versus unaccounted goal
  deltas.

## 4. Regression evidence

- `current_turn_prompt_uses_latest_user_message` proves a historical research
  question does not replace the current read instruction.
- `accounting_deltas_are_distinct_from_cumulative_limits` proves the separate
  accounting and hard-limit state shape.
- Existing loop terminal-event tests continue to require exactly one
  `AgentFinished` after tool-call turns.
- Targeted loop test suite: 40 passed.
- Agent-loop harness: 32 passed; 8 pre-existing fixture failures remain in
  authority/dispatcher/task tests. They report `principal mismatch:
  grant=build, context=agent:build` and related fixture timeouts, and do not
  involve turn identity, accounting, or terminal-event ownership.

## 5. Verification executed

- `rtk cargo fmt --all` — passed.
- `rtk cargo check -p codegg --tests` — passed.
- `rtk cargo test -p codegg --lib agent::r#loop::tests -- --nocapture` —
  passed (40 tests).
- `rtk cargo test --test agent_loop_harness -- --test-threads=1` — 32 passed,
  8 unrelated authority/fixture failures as described above.
- `rtk scripts/verify.sh quick` — passed.

## 6. Documentation and compatibility

`architecture/agent.md` and `architecture/goal.md` now document the
session-origin/current-turn split, cumulative versus unaccounted accounting,
and terminal event ownership. No frontend wire format, storage schema, or
migration was changed. Multi-turn routing and research behavior changes are
intentional correctness fixes.

## 7. Unresolved findings (severity: critical/high/medium/low)

- Critical/high/medium: none for M004.
- Low: the existing agent-loop harness has eight unrelated authority/fixture
  failures under the current baseline (`grant=build` versus
  `context=agent:build`, plus dependent dispatcher/task timeouts). This should
  be reconciled by the M001/M005 integration owners if it remains present; it
  is not a reason to reopen M004.

## 8. Downstream disposition

M005 is unblocked and is moved from `blocked` to `ready` in the same planning
update. Its hard dependencies M001, M002, and M004 are now closed; M003 is
only a soft dependency. M006 remains blocked on M005, and M009 remains blocked
on M001–M008. M007 and M008 remain independently ready.
