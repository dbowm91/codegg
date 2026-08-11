# Agent Runtime Correctness, Autonomy, and Simplification M004 — Turn Identity, Accounting, and Lifecycle Correctness

Status: ready

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M004

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: correctness invariant

Dependencies:

- hard: none
- interface: current `ChatRequest` message history, goal runtime/accounting APIs, daemon `TurnRuntime` lifecycle events
- soft: M005 depends on the corrected turn-state semantics

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md` — session, turn, run, goal
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/goal.md`
- `architecture/projection.md`

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/004-status.md`

## 1. Objective

Correct three small but consequential turn-state defects before the broader recovery/autonomy refactor:

1. distinguish the current user turn from the session-origin/first user message for routing, research hints, repository-task classification, and recovery;
2. separate cumulative hard execution counters from per-turn goal-accounting deltas so continuation turns do not repeatedly charge prior tool calls;
3. establish one authoritative owner for `AgentFinished` publication and preserve daemon `TurnCompleted`/`TurnFailed` semantics without duplicate terminal events.

## 2. Explicit non-goals

Do not:

- redesign goal persistence, budgets, scheduling, or completion evidence;
- change the semantic meaning of a session's original user goal where it is intentionally retained for context/security review;
- redesign event projection or frontend protocols broadly;
- remove useful `TurnCompleted`/`TurnFailed` daemon events merely because `AgentFinished` exists;
- merge session and turn identity into one type;
- introduce a durable per-turn accounting ledger if existing goal runtime storage accepts correct deltas;
- perform the M005 recovery-state-machine refactor here;
- alter model routing heuristics beyond feeding them the correct current turn.

## 3. Current implementation evidence

Inspect at minimum:

- `src/agent/loop.rs::run`, `extract_first_prompt_and_tool`, `apply_auto_routing`, `maybe_inject_research_hint`, repository-task/recovery helpers;
- `src/agent/turn_runtime.rs::latest_user_question` and request assembly;
- `AgentLoopState` counters;
- `account_goal_for_turn()` and `maybe_continue_goal()`;
- `goal::runtime::account_for_turn` and budget semantics;
- `AgentLoop::publish_agent_finished()`;
- `TurnRuntime` spawned-loop completion/error publication;
- consumers/tests for `AppEvent::AgentFinished`, `CoreEvent::TurnCompleted`, and `CoreEvent::TurnFailed`.

Known baseline defects:

- the loop derives `original_prompt` by scanning request messages from the beginning and taking the first user text; in a resumed/multi-turn session this can classify the current turn using stale historical input;
- research hints are prepended to the first user message found rather than necessarily the current user message;
- `turn_runtime.rs` already implements a `latest_user_question()` helper, proving that the current-turn distinction is available conceptually but not used consistently;
- `tool_call_count` is cumulative and participates in hard limits;
- `account_goal_for_turn()` passes the cumulative count to goal accounting;
- continuation code resets token deltas before a new autonomous continuation but not the cumulative tool-call counter, so prior calls can be charged again;
- `AgentLoop::publish_agent_finished()` publishes stop reason and usage from the actual provider events, while `TurnRuntime` also publishes `AgentFinished` after loop completion/failure with generic completion/error state and no usage.

## 4. Invariants that cannot regress

- session-origin goal/context remains available where semantically required;
- current-turn routing/research/recovery uses the latest user turn submitted for that execution, not arbitrary earlier history;
- injected hints/control text attach to the current turn or a system/control surface, not historical user messages;
- cumulative hard limits remain cumulative across the loop run;
- goal-accounting inputs represent only work not previously accounted;
- continuation turns cannot repeatedly charge prior tool calls or token usage;
- an accounting failure does not silently reset hard limits;
- exactly one `AgentFinished` is emitted per agent-loop terminal lifecycle unless a documented nested/subagent lifecycle intentionally emits its own distinct event;
- daemon `TurnCompleted`/`TurnFailed` remains emitted once per daemon turn;
- terminal stop reason/usage fidelity is not lost when duplicate publication is removed;
- frontend consumers do not receive a success event after a failed loop.

## 5. Target turn-state model

Use explicit names for distinct concepts. Representative fields:

```text
session_origin_prompt / user_goal
current_turn_prompt
cumulative_tool_calls
unaccounted_tool_calls_this_turn
last_provider_input_tokens_delta
last_provider_output_tokens_delta
```

The exact fields may differ. The key is that a cumulative limit counter is never reused as a delta.

Prefer computing current-turn input during `TurnRuntime` request assembly and passing it explicitly where practical. If the request history is the source of truth, use one shared helper that finds the latest user message, not several forward scans.

## 6. Current-turn identity requirements

Audit every helper that extracts a user prompt from full message history.

Classify each usage:

- **session-origin** — long-running user goal, context frame `user_goal`, security-review original task, historical metadata;
- **current-turn** — model routing, research trigger, repo-task classifier, narration/continuation recovery, turn-local telemetry;
- **latest unresolved instruction** — only if a distinct concept is truly required.

Requirements:

- name helpers/fields to reflect which category they return;
- do not use `find_map` from the beginning for current-turn behavior;
- research hint injection must modify the current submitted user turn or use a separate control message;
- repeated continuations should preserve the initiating current-turn identity rather than selecting a synthetic continuation prompt as the user's original task unless the continuation policy explicitly needs that synthetic prompt.

## 7. Goal accounting requirements

Separate hard-limit and accounting counters.

Preferred options:

- maintain `cumulative_tool_call_count` plus `tool_calls_since_last_accounting`;
- or snapshot the cumulative count and calculate a checked delta at accounting time.

Requirements:

- reset/account the delta exactly once after successful accounting or according to existing failure semantics;
- token counters must also represent actual provider-turn deltas. Confirm whether provider usage is per-response or cumulative before preserving `last_turn_*` naming;
- continuation accounting must not double-charge the user turn or earlier continuation turns;
- budget-limited wrap-up behavior remains bounded;
- hard `max_tool_calls` continues to use cumulative count;
- add a multi-continuation test with distinct tool counts per turn and assert exact budget totals.

## 8. Terminal lifecycle ownership requirements

Inventory all `AgentFinished` publishers and consumers.

Select one owner. Preferred:

- `AgentLoop` owns `AppEvent::AgentFinished` because it has actual terminal provider usage/stop reason;
- `TurnRuntime` owns daemon/core `TurnCompleted`/`TurnFailed` and should not emit a second `AgentFinished` for the same loop.

If repository consumer architecture makes `TurnRuntime` the better owner, move the accurate terminal summary out of the loop and return it; do not keep both.

Requirements:

- failure path emits one failure lifecycle event and no generic success;
- cancellation/interruption preserves its actual stop classification;
- nested/subagent events remain distinguishable by session/run identity;
- tests count terminal events rather than merely checking that at least one exists.

## 9. Ordered work packages

### Work package A — User-input identity inventory

1. find all message-history prompt extraction helpers;
2. classify each as session-origin/current-turn/other;
3. select one current-turn helper or explicit field;
4. update routing, research triggering, repo-task classification, and recovery inputs;
5. ensure context/security review still receives intentional session-origin goal.

### Work package B — Fix research/current-turn insertion

1. target the latest submitted user message for research hints, or inject the hint as a separate control instruction;
2. add a multi-turn test where turn 1 is a research question and turn 2 is not, then reverse the pattern;
3. assert only the current turn influences the trigger;
4. ensure historical messages are not mutated unexpectedly.

### Work package C — Separate accounting deltas

1. identify cumulative counters and provider usage semantics;
2. add per-accounting delta state;
3. update tool-call increments and accounting reset/snapshot behavior;
4. update continuation loop accounting;
5. add exact multi-turn budget assertions;
6. preserve hard-limit behavior.

### Work package D — Consolidate terminal event owner

1. list all `AgentFinished` publications;
2. identify frontend/subscriber expectations;
3. retain one authoritative publication with stop reason/usage;
4. keep `TurnCompleted`/`TurnFailed` once per daemon turn;
5. add event-count/fidelity tests for success, provider failure, and cancellation if practical.

### Work package E — Documentation

Update:

- `architecture/agent.md` turn state/lifecycle;
- `architecture/goal.md` accounting deltas if documented;
- `architecture/projection.md` only if terminal event ownership is described there.

Avoid documenting every internal counter; document semantic ownership.

## 10. Storage, protocol, migration, and compatibility effects

Storage:

- no schema migration expected;
- goal persisted totals become more accurate; existing historical totals are not retroactively rewritten.

Protocol:

- no external wire-format change expected;
- if `AppEvent` is internal, duplicate removal is internal behavior correction;
- if a frontend currently relies on duplicate `AgentFinished`, update that frontend/test to the one-event contract rather than preserving duplicates.

Compatibility:

- multi-turn sessions may route/research-trigger differently because they now use the current turn. This is intended correctness.

## 11. Concurrency, cancellation, and failure semantics

- accounting state is owned by one `AgentLoop` run and does not require global synchronization;
- cancellation should not cause the same delta to be charged twice during cleanup;
- if goal accounting fails, preserve enough state to avoid falsely reporting successful budget persistence; follow existing error/log policy rather than silently fabricating success;
- terminal event consolidation must remain race-safe when the agent loop is spawned and the daemon observes completion asynchronously.

## 12. Focused verification

Required focused tests:

```text
multi-turn current prompt selection
research trigger uses current turn only
routing/repo classifier uses current turn
session-origin goal remains stable
exact per-turn tool accounting across 2+ continuations
max_tool_calls remains cumulative
exactly one AgentFinished on success
exactly one AgentFinished on failure/cancel path as applicable
TurnCompleted/TurnFailed remain one-per-turn
terminal usage/stop reason fidelity retained
```

Then run:

```bash
scripts/verify.sh quick
```

Run focused goal/projection tests if shared event/accounting types change. No full workspace requirement unless event type ownership touches broad consumers.

## 13. Static guards

Do not add static scripts for `find_map` direction, counter names, or event publication counts.

These are behavioral invariants and belong in unit/integration tests.

## 14. Acceptance criteria

M004 closes only when:

- current-turn behavior no longer uses the oldest user message;
- research hints attach to the current turn/control surface;
- session-origin goal remains available separately;
- goal tool-call accounting uses deltas and exact multi-continuation tests prove no repeated charging;
- cumulative hard limits remain correct;
- one layer owns `AgentFinished` for a turn;
- duplicate generic terminal publication is removed;
- daemon `TurnCompleted`/`TurnFailed` behavior remains correct;
- stop reason/usage fidelity is retained;
- focused tests and `scripts/verify.sh quick` pass;
- no goal schema/protocol redesign is introduced.

## 15. Stop conditions

Stop and create a narrower follow-up if:

- provider usage semantics differ by provider in a way that requires normalization before per-turn accounting can be correct;
- external protocol consumers demonstrably require a versioned terminal-event migration rather than internal duplicate removal;
- current-turn identity cannot be determined reliably from the existing daemon turn input without changing the core turn protocol.

Do not broaden this milestone into a goal-system or projection redesign.

## 16. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/004-status.md` must include:

- implementation commit/PR;
- session-origin versus current-turn ownership summary;
- multi-turn routing/research regression evidence;
- exact goal accounting example/test showing no double charge;
- before/after terminal event publisher list;
- event-count and usage/stop-reason test results;
- quick verification outcome;
- unresolved findings classified by severity.