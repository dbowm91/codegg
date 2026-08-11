# Agent Runtime Correctness, Autonomy, and Simplification M005 — Agent-Loop Recovery and Autonomy State Machine

Status: blocked

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- Milestone M005

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Primary class: correctness/reliability simplification

Dependencies:

- hard: M001 MCP authority/provenance/tool-surface correctness
- hard: M002 textual tool-call repair safety
- hard: M004 turn identity/accounting/lifecycle correctness
- soft: M003 workspace-bound construction

Relevant references:

- `plans/000-long-term-specification.md`
- `plans/003-planning-process.md`
- `architecture/agent.md`
- `architecture/tool.md`
- existing model-profile/adaptation architecture

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md`

## 1. Objective

Replace the overlapping no-tool-call, narration, bootstrap, continuation, and repeated-failure branches in `AgentLoop` with one explicit bounded recovery/autonomy state machine.

The result should be easier to reason about, cheaper in provider turns, and more robust across strong and tool-fragile models without reducing the agent's ability to autonomously finish multi-step repository work.

## 2. Explicit non-goals

Do not:

- build a new workflow engine or durable planner;
- remove goal continuation, todos, subagents, tool programs, or normal autonomous repository work;
- replace the model with deterministic planning logic;
- force strong models through weak-model bootstrap behavior;
- infer new permissions during recovery or re-enable tools denied by user/profile policy;
- make every tool failure terminal;
- preserve every historical recovery counter/branch merely for compatibility;
- add provider-specific string checks throughout the generic loop;
- persist ordinary interactive recovery state to the database unless existing restart semantics already require it;
- add more prompts/nudges than the current system.

## 3. Current implementation evidence

Inspect at minimum:

- `src/agent/loop.rs` from provider response processing through the no-tool-call branch and tool-result recovery branch;
- `src/agent/progress_recovery.rs`;
- model execution policy/profile fields controlling bootstrap, parallelism, post-tool continue nudges, task-state policy, and tool exposure;
- `EventProcessor`, stop-reason normalization, and M002's provider-turn outcome normalization;
- tool result projection and error/denial handling;
- goal/todo completion state used to determine whether work is actually pending;
- tests covering missing structured calls, narration loops, bootstrap behavior, doom-loop/stall detection, post-tool continuation, and recovery palette restoration.

Baseline overlap to remove or reconcile includes:

- textual tool-call parsing when structured calls are absent;
- `RecoveryController` narration observation and nudge/correct/replan behavior;
- synthetic `list` of `.` bootstrap call for repository tasks;
- repeat bootstrap budget;
- post-tool continuation retry budget;
- generic `Continue working and use additional structured tool calls...` injection;
- narration retry budget;
- missing structured-tool-call retry budget;
- separate recovery-controller actions after actual tool results;
- model-specific execution-policy booleans that may duplicate state-machine decisions.

## 4. Invariants that cannot regress

- recovery is bounded by explicit attempt/turn/tool/time limits;
- a normal final answer with no explicit pending task is allowed to finish;
- malformed tool protocol receives at most the bounded adapter repair established by M002;
- user denial is never treated as a signal to restore or broaden authority;
- profile/agent-hidden tools remain hidden during recovery;
- base palette restoration may only restore the already-authorized profile-filtered surface, never denied tools;
- repeated identical tool calls/results without new evidence eventually stall rather than loop indefinitely;
- provider/network transient errors retain existing bounded retry semantics and are not conflated with model no-progress recovery;
- cancellation and user steering take precedence over autonomous continuation;
- explicit active goals/todos may justify continuation; natural-language narration alone should not create unlimited extra work;
- strong/structured-call models should take the shortest path through the state machine with no synthetic bootstrap turns;
- a stalled outcome produces actionable evidence rather than silent completion.

## 5. Target state machine

Use an explicit internal enum/state object rather than several independent retry counters. The exact naming is flexible, but the behavior should resemble:

```text
Provider outcome
  |
  +-- valid tool calls --------------------> ExecuteTools
  |
  +-- final text + no explicit pending work -> Finish
  |
  +-- malformed tool protocol ------------> AdapterRepair (bounded once)
  |
  +-- soft stop + explicit pending work ---> ContinueOrReplan (bounded)
  |
  +-- repeated no-progress ----------------> Stall

Tool execution outcome
  |
  +-- progress/new evidence ---------------> Continue normal loop
  +-- transient/recoverable failure -------> Correct/Replan (bounded)
  +-- denied/cancelled ---------------------> Respect authority / report blocker
  +-- repeated equivalent failure ---------> Stall
```

The state machine should own retry counts and transitions. Do not keep parallel counters that independently decide to `continue` the provider loop.

## 6. Pending-work requirements

Autonomous continuation should prefer explicit state:

- active long-horizon goal with continuation decision;
- todo item still in progress/pending where policy expects execution;
- just-executed tool result that clearly requires a provider follow-up;
- malformed protocol repair explicitly requested by M002;
- provider stop/truncation condition known to require continuation.

Natural-language heuristics such as `indicates_more_work(text)` may remain as a weak signal only if necessary, but they must not alone produce a cascade of synthetic calls/retries. Prefer using them to improve a diagnostic or choose between `Finish` and a single `ContinueOrReplan` transition when explicit task state also supports continuation.

## 7. Bootstrap policy requirements

The generic loop should not synthesize `list { path: "." }` solely because the prompt appears repository-related.

Disposition:

1. measure/inspect which supported model profiles require bootstrap behavior;
2. if no active profile requires it, delete synthetic bootstrap entirely;
3. if a weak/tool-fragile model demonstrably needs bootstrap, move it into a resolved profile/adapter policy with an explicit maximum of one initial read-only bootstrap action;
4. bootstrap must use the explicit workspace root semantics from M003 and current tool surface;
5. bootstrap cannot bypass plan mode, permissions, disabled tools, or tool availability;
6. do not repeat bootstrap several times because narration persists.

## 8. Typed tool outcome requirements

Where practical, stop making recovery infer all status from rendered result strings.

Introduce or reuse a compact typed status returned by the execution layer, for example:

```text
ToolExecutionStatus = Success | Denied | Timeout | Cancelled | ToolError | ProtocolError
ToolExecutionOutcome { status, model_text, optional metadata }
```

Requirements:

- preserve the current model-facing projected text;
- recovery consumes typed status plus bounded fingerprints/progress metadata;
- permission denial is distinguishable from tool implementation failure;
- timeout/cancellation are distinguishable from ordinary error text;
- Tool Program/broker integration should reuse existing normalized result types where possible rather than create an incompatible duplicate;
- do not require every tool implementation to return a new rich domain object if one adapter at the registry/broker boundary can normalize current results safely.

## 9. Recovery-controller disposition

Keep `RecoveryController` if it provides useful bounded incident classification, but make it a component of the single state machine rather than a second independent decision engine.

Audit each `RecoveryAction`:

- `Nudge` — retain only as one bounded control instruction;
- `Correct` — retain for malformed/known invalid structured calls;
- `RestoreBasePalette` — retain only for policy-induced starvation and never restore explicitly denied tools;
- `ConstrainParallelism` — retain only if an actual incident can select it and tests prove value; otherwise delete dead/unused action;
- `Replan` — retain as one transition, not an additional retry family;
- `Stall` — final bounded failure/stop state.

Delete actions/config fields that no longer have a reachable distinct transition.

## 10. Ordered work packages

### Work package A — Build behavior inventory

1. enumerate all loop branches that call `continue`, inject a recovery instruction, synthesize a tool, or reset retry counters after a provider response;
2. map each branch to the failure condition it is intended to solve;
3. identify duplicate conditions/actions;
4. capture focused tests representing supported behavior before refactor;
5. explicitly list model profiles relying on bootstrap or narration repair.

### Work package B — Define normalized recovery input

1. consume M002's normalized provider-turn outcome;
2. normalize tool results into typed status where feasible;
3. represent explicit pending-work state from goal/todo/continuation context;
4. keep bounded fingerprints/progress evidence from `RecoveryController`;
5. avoid copying raw large tool output into recovery state.

### Work package C — Implement one state object

1. replace independent retry budgets with one recovery state containing total attempts and current incident;
2. encode transitions explicitly;
3. guarantee a maximum number of repair/continue transitions per provider/tool incident;
4. reset recovery only on observable progress/new evidence, not merely any model response;
5. make `Stall` produce one clear error/report and exit the recovery path.

### Work package D — Remove synthetic/duplicate branches

1. remove unconditional/repeated synthetic `list .` bootstrap or move the one retained weak-model case into profile policy;
2. remove duplicate post-tool generic continue messages when the state machine already schedules continuation;
3. remove separate narration/missing-tool counters subsumed by the state object;
4. delete obsolete execution-policy booleans/config fields if no longer consumed and compatibility permits;
5. delete comments/docs describing removed branches.

### Work package E — Recovery authority tests

Add tests proving:

- denial does not broaden/restored authority;
- hidden/disabled tool remains unavailable after palette recovery;
- repeated same call/result stalls within bound;
- malformed textual protocol receives only M002 repair allowance;
- strong model final answer finishes without bootstrap;
- weak-model bootstrap, if retained, occurs at most once and only for the assigned profile;
- explicit active goal/todo may continue after a soft stop;
- cancellation/steer interrupts recovery;
- provider retry remains separate from model recovery.

### Work package F — Documentation and observability

Update `architecture/agent.md` with state-machine semantics and remove stale doom-loop/bootstrap narratives.

Tracing should record compact fields such as incident kind, transition, attempt count, and stall reason. Do not log raw sensitive arguments/results merely for recovery diagnostics.

## 11. Storage, protocol, migration, and compatibility effects

Storage/protocol:

- none expected.

Config compatibility:

- if obsolete recovery booleans/config fields are public config keys, prefer deprecating/ignoring them with a warning for one compatibility window rather than failing config parsing;
- if fields are entirely internal/default-only, delete directly.

Behavior compatibility:

- supported fragile models retain a bounded compatibility path;
- accidental multiple bootstrap/narration retries are intentionally removed;
- strong models should experience fewer extra provider/tool turns.

## 12. Concurrency, cancellation, and failure semantics

- recovery state is turn-local;
- parallel tool execution remains governed by existing execution policy/resource controls;
- any temporary `ConstrainParallelism` transition restores normal limits only after progress or incident resolution;
- cancel/steer checks occur before another autonomous provider/tool action;
- provider request retry keeps its own network error budget and must not consume model-recovery attempts unless the provider returns a semantic model outcome;
- background subagent/security review scheduling remains outside this state machine unless its result directly feeds current-turn pending work.

## 13. Focused verification

Create a deterministic scripted/mock-provider table covering at least:

```text
final answer -> finish
structured call -> execute -> final
malformed tool protocol -> one repair -> success
malformed protocol -> failed repair -> stall/final error
narration with no explicit pending work -> finish
soft stop + explicit pending todo/goal -> one continuation
repeated identical failed tool -> bounded recovery -> stall
denied tool -> no authority broadening
palette starvation -> authorized base palette only
cancel during recovery -> stop
weak-profile bootstrap if retained -> at most once
strong profile -> no synthetic bootstrap
```

Then run:

```bash
scripts/verify.sh quick
```

Run focused goal/tool-program/subagent tests only where shared normalized outcomes or execution-policy types changed.

## 14. Static guards

Do not add a static loop-complexity threshold or regex guard for retry counters.

The desired closure evidence is deletion of duplicate branches plus deterministic state-machine tests.

## 15. Acceptance criteria

M005 closes only when:

- no-tool-call/recovery behavior is owned by one explicit bounded state machine;
- independent narration/bootstrap/post-tool/missing-call retry counters are removed or reduced to state-machine fields;
- generic repeated synthetic `list .` behavior is removed; any retained bootstrap is profile-specific and at most once;
- M002 adapter repair is the only textual-tool protocol repair path;
- recovery uses typed tool status where practical and distinguishes denial/timeout/cancel/error;
- palette restoration cannot restore explicitly denied tools;
- repeated equivalent no-progress reaches `Stall` within a tested bound;
- cancellation/steering interrupts recovery before new autonomous work;
- strong models incur no unnecessary repair/bootstrap turns in normal fixtures;
- focused recovery table and `scripts/verify.sh quick` pass;
- net code/branch complexity in the generic no-tool path decreases rather than being relocated into another equivalent layer.

## 16. Stop conditions

Stop and split a follow-up if:

- a provider/model family requires a fundamentally different hosted continuation protocol that cannot fit the normalized outcome contract;
- typed tool status would require rewriting every tool implementation rather than adding a narrow normalization boundary;
- goal continuation semantics require a separate durable scheduler redesign.

Do not preserve overlapping generic repair logic to accommodate one unclassified model. Put model quirks behind an adapter/profile contract.

## 17. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md` must include:

- implementation commit/PR;
- before/after list of recovery branches/counters;
- final state-machine transition table;
- disposition of synthetic bootstrap and each `RecoveryAction`;
- typed tool-outcome/status disposition;
- deterministic scripted-provider recovery test results;
- quick verification outcome;
- evidence that no permission/tool-surface broadening occurs during recovery;
- remaining model-specific compatibility limitations by severity.