# Agent Runtime, Model Adaptation, and ACP Milestone 006 — Progress, Loop, and Tool Recovery Controller

Status: blocked — requires Milestone 002 closure; final integration requires Milestone 003

Repository baseline: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Source roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-006--progress-loop-and-tool-recovery-controller`

Long-term requirements:

- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#28-observability`
- `plans/000-long-term-specification.md#29-system-invariants`

Primary class: reliability

## 1. Objective

Replace the current narrow repeated-identical-tool-call detector with a bounded observable progress and recovery controller. The controller must gently redirect an agent after the first detected no-progress pattern, adapt the available execution surface or control-message placement when appropriate, and terminate only after graduated recovery fails.

The controller must reason exclusively from observable agent actions, tool calls, tool results, errors, files/evidence changed, and runtime state. It must not inspect, infer, persist, or expose hidden chain-of-thought.

## 2. Dependencies

Hard dependency:

- M002 resolved capability/tool surface, including base-surface restoration and canonical/wire tool mapping.

Integration dependency:

- M003 descendant lineage/cancellation must be represented before this milestone closes, so recovery budgets and terminal stalled outcomes propagate correctly through nested agents.

Future dependency:

- M007 adapters will provide model-specific control-message placement and recovery preferences. M006 must define a generic typed seam with conservative defaults.

## 3. Current implementation evidence

Re-audit:

- `DoomLoopDetector` records a canonicalized hash of tool name and JSON arguments in a sliding window;
- it triggers when the most recent exact key reaches a threshold;
- permission checking records tool calls and may reject a doom loop;
- the agent loop also handles text-to-tool fallback, missing structured calls, provider retry, context-palette reduction, tool timeouts, and provider/model control instructions;
- current model profiles include fields such as explicit tool contract, post-tool continue nudge, late-system support, and user-control preference;
- there is no unified representation of equivalent results/errors, short cycles, narration-without-action, malformed repeated calls, or measurable task progress.

## 4. Invariants

- Recovery uses observable actions/results only.
- A single repeated action does not immediately kill a valid iterative workflow.
- First response is a bounded corrective nudge, not a broad reset.
- Recovery cannot grant new permissions or tools beyond the resolved base surface.
- Restoring a palette means restoring the already-authorized base surface, not bypassing policy.
- Canonical tool aliases are corrected through M002 mapping before execution.
- Recovery state is bounded per root/agent run and cannot grow with transcript length.
- Terminal stalled outcomes include concrete observable evidence and do not pretend success.
- Parent/child cancellation and budgets remain authoritative.
- Recovery control content is private runtime guidance and follows provider/model adapter placement policy.

## 5. Scope

### In scope

- Define `ProgressObservation`, `ProgressSignal`, `RecoveryIncident`, `RecoveryAction`, and `StalledReport` types.
- Record bounded observations per turn/tool batch:
  - canonical/wire tool name;
  - normalized argument fingerprint;
  - result fingerprint and size class;
  - error class/code;
  - files/symbols/evidence newly observed;
  - files/state changed;
  - assistant action class: structured call, malformed call, narration only, final answer;
  - selected tool-surface fingerprint;
  - descendant activity changes.
- Detect:
  - exact repeated calls;
  - equivalent-result repetition;
  - equivalent-error retries with cosmetic argument changes;
  - short two-node/three-node cycles;
  - repeated malformed or unknown tool calls;
  - narration of intended tool use without a structured action;
  - repeated calls to unavailable/omitted tools;
  - no-progress tool sequences;
  - palette starvation where required/recently requested tools were omitted;
  - runaway child-spawn attempts or repeated delegation rejection.
- Implement a configurable graduated recovery ladder.
- Integrate generic control-message placement seam for M007.
- Restore full base tool surface or reduce parallelism when indicated.
- End with a typed stalled result only after bounded attempts.
- Emit bounded diagnostics/events for incidents and actions.

### Out of scope

- Semantic analysis of private model reasoning.
- Automatic model switching/fallback beyond an explicit existing typed provider failure path.
- Arbitrary self-modification of prompts or adapter definitions.
- Long-term learning from user conversations.
- Unbounded retry or retry-until-success.
- Provider-specific recovery behavior beyond a typed seam.
- Replacing normal provider retry for network/rate-limit errors.

## 6. Required production changes

### Observation model

Use stable hashes and bounded summaries. Normalize known non-semantic argument fields cautiously (for example whitespace or generated call IDs) only where tool contracts define equivalence. Do not collapse materially different commands/paths.

Result fingerprints should combine canonical tool, terminal status/error class, bounded normalized summary, and relevant state delta. Keep recent history in a fixed-size ring.

### Progress model

Positive progress may include:

- new file/symbol/evidence discovered;
- changed diagnostic hypothesis grounded in a different result;
- successful state mutation expected by the task;
- child lifecycle advancement;
- resolved todo/task phase;
- different tool/backend used to test a hypothesis.

Repeated reads may be valid when content changed; compare content/result fingerprint rather than tool name alone.

### Recovery ladder

Provide conservative defaults, for example:

1. **Nudge:** explain the observable repeated pattern and require a different action or explicit blocker report.
2. **Correct:** supply canonical tool name/schema/alias or indicate the actual available alternatives.
3. **Restore/constrain:** restore base tool palette, disable parallel tool calls temporarily, or require one structured action.
4. **Replan:** ask for a short observable plan grounded in latest results, without exposing hidden reasoning.
5. **Stall:** stop with typed report containing last progress, repeated pattern, attempted recoveries, blocking errors, and suggested user action.

Exact thresholds may vary by incident type and model adapter, but must be bounded and testable.

### Agent-loop integration

- Observe after provider completion, tool normalization, tool result, and descendant lifecycle changes.
- Avoid recording one logical parallel batch as serial repeated failures incorrectly.
- Recovery messages enter the next provider request through the canonical prompt/control context seam.
- Clear or decay incidents after demonstrated progress.
- Do not reset the entire conversation or discard valid state.

### Nested agents

Recovery budgets are per lineage node with a bounded root aggregate. Repeated rejected child spawns should nudge the parent toward an allowed target or direct execution. Parent cancellation remains distinct from stalled termination.

### Events/projection

Add bounded recovery-status events or diagnostics only if existing turn/progress events cannot represent them. User-visible surfaces may show a concise “recovery nudge” or “stalled” status; never expose private reasoning.

## 7. Ordered work packages

### A — Taxonomy and fixtures

- inventory current loop/tool fallback behavior;
- define error/action/progress classes and bounds;
- add deterministic fixtures for exact repeat, A-B cycle, equivalent error variants, narration only, missing tool, palette starvation, and valid repeated polling.

### B — Observation/progress engine

- implement bounded ring state and stable fingerprints;
- integrate file/evidence/descendant deltas;
- distinguish parallel batches and valid changed-result polling;
- expose pure decision tests.

### C — Recovery actions

- implement nudge/correct/restore/constrain/replan/stall actions;
- route control messages through generic placement policy;
- integrate M002 base-surface restoration and canonical aliases;
- reduce temporary parallelism without mutating global config.

### D — Agent-loop and descendant integration

- observe every relevant lifecycle point;
- propagate stalled terminal state to task/parent;
- ensure cancellation and provider retry remain separate;
- bound root/child recovery budgets.

### E — Documentation and observability

- document incident taxonomy, thresholds, and operator diagnostics;
- add concise event/TUI status behavior;
- retire or wrap `DoomLoopDetector` so only one recovery authority remains.

## 8. Failure, cancellation, restart, and contention semantics

- Recovery state is in-memory per active lineage unless existing durable turn metadata can store a bounded checkpoint cheaply; restart may reset incidents but must not resurrect cancelled work.
- Cancellation always wins over recovery/replan.
- Provider network retry does not count as agent no-progress unless the model repeatedly receives/causes the same terminal tool error afterward.
- Parallel tool calls are evaluated as one batch plus individual outcomes.
- Locking around observations is short and contains no awaits.
- Stalled termination releases child/tool/scheduler resources through ordinary completion paths.
- A recovery-controller internal error falls back to conservative existing limits rather than allowing unbounded execution.

## 9. Compatibility

- Existing exact-repeat thresholds may map into default recovery configuration.
- Existing provider retry/backoff remains intact.
- Existing text-to-tool parser remains available but repeated malformed recovery is now governed centrally.
- Existing model-profile recovery booleans become generic policy inputs until M007 migrates them.
- Existing user steering/cancellation remains higher priority than automatic nudges.

## 10. Required tests

Focused:

- exact-repeat detection;
- equivalent-result and equivalent-error detection;
- A-B and A-B-C cycle detection;
- valid repeat with changed result is progress;
- narration without call;
- malformed/unknown tool correction;
- canonical alias correction;
- missing tool and palette restoration;
- temporary parallelism reduction;
- progress clears/decays incidents;
- threshold and ring bounds;
- terminal stalled report fields;
- cancellation precedence;
- child spawn rejection recovery.

Production-shaped:

- mock model repeats a read, accepts first nudge, then uses grep and completes;
- mock model retries equivalent shell errors, receives schema/tool correction, and reports blocker;
- palette reduction omits a needed tool, controller restores base surface, next call succeeds;
- nested child stalls and parent receives typed result without looping;
- valid long-running test/status polling with changing output does not trigger false positive.

Negative/security:

- recovery cannot add parent-denied tools;
- control messages do not include secret tool arguments/results;
- hidden reasoning is absent from observations/events;
- malicious tool output cannot inject a privileged recovery action;
- stalled report remains bounded.

## 11. Verification commands

```bash
cargo fmt --all -- --check
cargo test doom_loop
cargo test agent::loop
cargo test agent::tool_program_recovery
cargo test --test agent_loop_harness
cargo test --test subagent
cargo check --workspace
```

Add one focused recovery integration target using mock providers/tools. Run one broad local library suite; do not add live-model CI or repeated stress loops to routine CI.

## 12. Acceptance criteria

- Recovery covers exact repeats, equivalent errors/results, short cycles, malformed/no-action turns, missing tools, palette starvation, and repeated delegation rejection.
- First incidents nudge rather than terminate.
- Recovery can correct aliases/schema, restore authorized tools, and temporarily constrain execution.
- Demonstrated progress clears/decays incidents.
- Repeated failure ends in a typed bounded stalled outcome.
- Nested-agent and cancellation paths remain correct.
- No hidden reasoning is inspected or exposed.
- Existing network retry and user steering behavior remain intact.

## 13. Stop conditions

Stop if:

- progress classification requires hidden reasoning access;
- correct recovery requires bypassing permissions or permanently mutating global tool config;
- provider-specific message placement cannot be represented as a generic seam for M007;
- descendant stalled state cannot propagate through M003 task ownership;
- reliable fingerprints require storing unbounded tool outputs;
- scope expands into automatic model benchmarking or learned adaptation.

## 14. Closure evidence

Include:

- incident/action taxonomy and default thresholds;
- fixture matrix with expected recovery action;
- false-positive evidence for changing status polling;
- base-surface restoration and alias correction evidence;
- nested stalled/cancellation evidence;
- bounded memory/state evidence;
- focused and broad local verification results;
- closure recommendation.
