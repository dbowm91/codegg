# Agent Runtime Correctness Milestone 012 — Host-Owned Goal Completion Verification

Status: implemented

Repository baseline: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source roadmap:

- `plans/subsystems/agent-runtime-goal-verification-addendum.md#6-milestone`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. Preserve the existing daemon/scheduler/tool authority boundaries.

Primary class: invariant

## 1. Objective

Replace model self-certification of goal completion with one host-owned verification boundary that derives authoritative evidence from CodeGG-owned state, returns a typed verdict, and only then permits the goal store to transition to `Complete`.

## 2. Why this milestone is ready

All hard prerequisites already exist and are closed:

- durable goal state, budgets, checkpoints, and continuation semantics;
- scheduler job/attempt persistence and typed terminal states;
- durable delegated agent run ownership;
- existing tool results, todo state, Git/workspace inspection, and snapshot seams;
- bounded recovery/autonomy behavior and explicit user cancellation/steering precedence.

No new runtime, plugin framework, scheduler, or workflow engine is required.

## 3. Current implementation evidence

At baseline:

- `GoalRequestCompletionTool` in `src/tool/goal.rs` accepts model-provided `evidence`, `files_changed`, `tests_run`, and `remaining_risks`.
- after checking that evidence is non-empty and that either tests or an explicit skipped-test risk is present, it calls `GoalStore::update_status(&goal.id, GoalStatus::Complete)` directly.
- `GoalStatus` already distinguishes `Active`, `Paused`, `AwaitingUser`, `BudgetLimited`, `Complete`, `Failed`, and `Cancelled`.
- the runtime already has `should_continue` logic and goal progress/checkpoint plumbing.
- `JobRecord` carries `session_id`, `turn_id`, lineage, state, and bounded labels; `AgentRunRecord` carries durable job/run provenance.
- `GoalCompleted` is published as an application event after the direct completion transition.

The gap is therefore the terminal certification boundary, not missing persistence or orchestration.

## 4. Invariants that must not regress

- The working model cannot directly transition an active goal to `Complete`.
- Goal completion remains a state-machine operation owned by core/domain code.
- Model-supplied evidence fields are non-authoritative claims when CodeGG can derive the same fact from owned state.
- A failed required scheduler/test/run fact cannot be overridden by prose.
- Verification itself is read-only with respect to workspace/tool execution.
- User cancellation, pause, steering, and budget exhaustion take precedence over completion/continuation.
- Verification failure cannot create an unbounded retry or continuation loop.
- Plugin presence or failure cannot weaken core verification.
- Existing daemon scheduler and run stores remain the only execution authorities.

## 5. Scope

### In scope

- introduce a typed `GoalCompletionProposal` representing the model request;
- introduce a typed `GoalVerificationVerdict`, at minimum `Met`, `NotMet`, and `AwaitingUser`;
- introduce a `GoalVerificationService` or equivalent core-owned boundary;
- derive structured evidence from existing stores/services where available;
- correlate active-goal work with jobs/runs using host-owned provenance rather than model text;
- make `goal_request_completion` submit a proposal and apply the verifier verdict;
- feed a bounded `NotMet.next_action`/evidence-gap summary into the existing goal continuation mechanism;
- make terminal transition concurrency-safe against stale proposals;
- update goal events/docs/tests to reflect host-accepted rather than model-certified completion.

### Explicitly out of scope

- an LLM verifier in this milestone;
- arbitrary plugin-defined completion authority;
- a new workflow/planner engine;
- new background schedulers;
- redesign of goal budgets/todos/run groups/worktrees;
- automatic execution of additional tests merely because completion was requested;
- adding new CI lanes, coverage gates, benchmark gates, or release automation.

## 6. Required production changes

### Core/domain

Add canonical proposal/verdict types under `crates/codegg-core/src/goal/` or the narrowest adjacent goal module. The verifier API should take stable goal identity plus a bounded evidence context assembled by the application layer; it must not accept raw access to arbitrary mutating tools.

The verdict should preserve criterion-level or evidence-gap detail sufficiently for deterministic continuation and operator inspection without storing unbounded model prose.

`GoalStore` should expose or use an atomic/status-checked terminal transition so a verifier result for an earlier active revision cannot complete a goal that has since been paused, cancelled, replaced, or otherwise transitioned.

### Evidence and provenance

Create one application-level evidence assembler that queries existing owners rather than copying their state machines. Prefer typed job/run/todo/Git/snapshot records.

At minimum distinguish:

- observed structured evidence;
- claimed model evidence;
- unavailable evidence;
- failed evidence.

Where scheduler/delegated work needs goal correlation, attach the active goal ID from host state when the job/run is created. Prefer existing bounded metadata when sufficient. Do not derive the relation from display text, prompt parsing, job ID formatting, or subagent names.

If labels prove insufficient for restart-safe/audit-safe correlation, stop and add one explicit typed provenance field with a normal migration rather than building a parser convention around labels.

### Storage and migrations

Avoid a new verification table unless repository evidence demonstrates that the existing goal record/checkpoint plus existing job/run stores cannot reconstruct the needed verdict evidence after restart.

If a goal revision/CAS field is required, use the smallest durable migration that makes stale completion impossible and document compatibility.

### Runtime and concurrency

Change `GoalRequestCompletionTool::execute` so it no longer directly marks the goal complete. The required shape is:

1. load and validate the current active goal;
2. construct `GoalCompletionProposal` from bounded model claims;
3. assemble CodeGG-owned evidence;
4. run deterministic verification;
5. revalidate goal status/revision;
6. on `Met`, apply the canonical complete transition and publish normal events;
7. on `NotMet`, update bounded progress/next-action state and allow the existing continuation controller to decide whether another turn is permitted;
8. on `AwaitingUser`, transition/use existing awaiting-user semantics rather than fabricating progress.

Do not add a second autonomous loop inside the verifier.

### Protocol and DTOs

Keep existing public goal events compatible if possible. If a verification summary is exposed, add only bounded optional fields or a separate compatible event/DTO; do not break existing clients merely to rename internal ownership.

### Plugin interaction

A future plugin evidence seam may contribute bounded observations, but this milestone should either omit it or make it optional/fail-closed with respect to its own declared check. Core verification must have a complete safe default with no installed plugins.

### Documentation and static guards

Update `architecture/goal.md` and any agent/tool architecture text that still describes model-owned completion.

Add a focused source-level or unit guard only if needed to prevent `GoalRequestCompletionTool` from directly invoking an unconditional `GoalStatus::Complete` transition in the future. Prefer type/API ownership over regex scripts.

## 7. Ordered work packages

### Work package A — Typed proposal/verdict and atomic transition boundary

Intent: make direct model-owned completion structurally impossible.

Required changes:

- define proposal/verdict types;
- establish verifier interface;
- provide stale-status/revision protection on terminal transition;
- refactor goal tool to call the boundary.

Acceptance evidence:

- unit tests show a proposal alone cannot complete a goal;
- stale proposal after pause/cancel is rejected;
- `Met` through the verifier produces the existing terminal event/state.

### Work package B — Authoritative evidence assembly

Intent: replace trust in model claims with CodeGG-owned facts where available.

Required changes:

- correlate scheduler jobs and delegated runs to the active goal using host provenance;
- expose bounded evidence from tests/builds/runs/todos/Git/snapshots as appropriate to current implementation;
- distinguish unavailable from failed evidence.

Acceptance evidence:

- a model claim that a recorded failed test passed cannot yield `Met`;
- changed-file/test/run evidence is derived from owned records in integration fixtures;
- restart reconstruction does not require in-memory verifier state.

### Work package C — Existing continuation integration

Intent: verification failure should resume bounded goal work rather than terminate or spawn a parallel workflow.

Required changes:

- map `NotMet` into existing goal progress/next-action representation;
- preserve budget/steering/cancellation precedence;
- use `AwaitingUser` when missing evidence requires user action.

Acceptance evidence:

- `NotMet` produces one bounded next action consumed by the existing continuation path;
- budget-limited/cancelled/paused goals do not continue because the verifier requested more work;
- repeated identical failed verification does not create an unbounded synthetic loop.

### Work package D — Documentation and closure-oriented integration tests

Intent: make ownership visible and test the real path.

Required changes:

- update architecture docs;
- add focused end-to-end tests around `goal_request_completion` with fake/durable job evidence;
- remove stale wording that says the model marks the goal complete.

Acceptance evidence:

- documentation identifies the goal state machine/verifier as terminal authority;
- tests exercise real tool-to-verdict-to-state behavior.

## 8. Failure, cancellation, restart, and contention semantics

A verifier error is not success. Return a bounded failure/`AwaitingUser` or leave the goal active with diagnostic state according to existing error conventions; do not silently complete.

Concurrent completion proposals for the same goal must not both independently mutate terminal state. The first valid terminal transition wins; later stale proposals observe terminal/current state and become no-ops or typed conflicts.

Cancellation/pause occurring during verification wins before terminal commit. Verification should be cancellable if it performs async store queries, but cancellation must not leave a partially updated goal.

Daemon restart reconstructs verifier inputs from durable goal/job/run records. No in-memory evidence cache is authoritative.

## 9. Compatibility and migration

User-visible semantics become stricter but compatible: `goal_request_completion` still requests completion, and successful completion still produces the existing goal terminal state/event.

Model prompts/tool descriptions should be updated to say “request completion” rather than imply self-certification.

Avoid schema migration unless required for stale-transition protection or durable goal-to-job provenance. If added, preserve old records by treating missing provenance as unavailable evidence rather than synthesizing a relation.

## 10. Required tests

### Focused unit tests

- proposal construction and bounds;
- verdict evaluation for pass/fail/unavailable evidence;
- stale goal status/revision rejection;
- model claims cannot override structured failed evidence.

### Integration tests

- `goal_request_completion` -> evidence assembly -> `Met` -> terminal event;
- failed test/delegated run -> `NotMet`;
- active unfinished todo/required work blocks completion when applicable;
- `NotMet` reaches existing continuation state without a second loop.

### Restart and recovery tests

- verifier can reassemble required durable evidence after recreating services;
- legacy records with no goal provenance fail conservatively where correlation is required.

### Contention and cancellation tests

- concurrent completion proposals;
- pause/cancel racing verifier completion;
- goal replacement/clear during verification.

### Security and negative tests

- plugin/model prose cannot elevate failed evidence;
- verifier has no mutating tool authority;
- untrusted textual evidence is stored/rendered as data, not executed.

## 11. Required verification commands

```bash
cargo test -p codegg-core goal
cargo test goal_request_completion
cargo test goal
scripts/verify.sh quick
```

Adjust focused test selectors to actual module/test names after implementation. Do not add a full-workspace gate unless changed code requires it under existing repository policy.

## 12. Documentation updates

- `architecture/goal.md`
- `architecture/agent.md` if continuation ownership is described there
- tool documentation for `goal_request_completion`
- planning closure record under `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`

## 13. Acceptance criteria

- No production path allows the working model to directly set an active goal to `Complete`.
- Host-owned deterministic verification runs before the terminal transition.
- CodeGG-owned job/run/test/workspace evidence overrides conflicting model claims.
- Failed or missing required evidence produces a bounded non-terminal verdict.
- Existing continuation, budget, cancellation, pause, and steering semantics remain authoritative.
- Restart does not require an in-memory verifier cache.
- Existing goal clients remain compatible or receive a documented additive protocol change.
- Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and report rather than improvise when:

- correct verification would require a second scheduler/workflow engine;
- goal-to-job provenance cannot be made restart-safe without a schema/authority decision larger than this plan;
- existing stores cannot distinguish authoritative test/run evidence from display text;
- completion semantics require changing unrelated run-group/worktree/plugin architecture;
- a proposed plugin seam would make plugin presence part of core completion correctness.

## 15. Closure evidence required

The closure record must include:

- implementation commit(s);
- proof that direct model-owned completion was removed;
- requirement-to-test mapping for `Met`, `NotMet`, `AwaitingUser`, stale proposal, cancellation, and restart cases;
- evidence-source inventory showing which claims are host-derived versus model-supplied;
- any storage/protocol migration and compatibility evidence;
- exact focused verification commands actually run and outcomes;
- `scripts/verify.sh quick` outcome;
- unresolved findings with severity and explicit disposition.

## 16. Handoff notes

Preserve the closed M001-M011 history. This is a new narrow follow-up, not justification to reopen recovery/prompt/tool authority work.

Implement deterministic verification first. Do not add an LLM verifier merely because some completion criteria are semantic; record such criteria as unavailable/awaiting-user or deferred unless there is a bounded deterministic interpretation already present in CodeGG.
