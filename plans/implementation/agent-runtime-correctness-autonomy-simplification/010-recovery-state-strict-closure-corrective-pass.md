# Agent Runtime Correctness, Autonomy, and Simplification M010 — Recovery-State Strict Closure Corrective Pass

Status: implemented

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- corrective follow-up to M005 and the M009 closure attempt

Original milestones and closure evidence under correction:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/005-agent-loop-recovery-and-autonomy-state-machine.md`
- `plans/closure/agent-runtime-correctness-autonomy-simplification/005-status.md`
- `plans/implementation/agent-runtime-correctness-autonomy-simplification/009-integration-documentation-and-closure.md`
- PR #74 (`agent/agent-runtime-m009-closure`) and its candidate `plans/closure/agent-runtime-correctness-autonomy-simplification/009-status.md`

Repository baseline reviewed:

- `5449aa2f589aa10d4e6eeda439b97d426506c759` on `main`
- PR #74 head reviewed: `7ae157e9c482760dac5c68b91146c5d36ad60a9a`
- PR #74 green hosted candidate referenced by its closure record: `c51547011bab6d44b41f1ce3cc0a2aec8ddf28f0`, hosted run `31515706555`

Primary class: corrective invariant/polish closure

Dependencies:

- hard: M001-M008 remain closed and are not reopened except where this plan explicitly validates their integration boundary;
- interface: retain the M009 broker-principal correction and workspace-fixture corrections from PR #74 or an equivalent rebased implementation;
- operational: one normal hosted `verify` run must pass on the final corrective merge candidate before strict closure.

Target closure record:

- `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md`

## 1. Objective

Strictly finish the recovery/autonomy simplification promised by M005 and prevent the M009 closure record from accepting a tree that still contains duplicate, unreachable, or authority-bypassing recovery branches.

This is a narrow deletion-and-correction pass. It is not a second recovery redesign. The desired end state is one visible, bounded autonomy/recovery authority shared by the primary and follow-up loops, with no dead bootstrap/narration branches and no standalone repository-specific continuation path that can create an extra provider turn outside the state-machine budget.

The pass also completes the typed-outcome intent of M005 at the narrowest useful boundary: when tool execution already knows whether a call succeeded, was denied, timed out, was cancelled, or failed, recovery must consume that typed status rather than rediscovering it from model-facing rendered text. String classification may remain only as an explicitly documented compatibility fallback where no typed status exists yet.

## 2. Why a corrective pass is required

The M005 closure record states that recovery decisions have one turn-local bounded owner, generic bootstrap is disabled, and the overlapping generic continuation/retry machinery was removed. The reviewed production tree does not fully satisfy those claims.

Concrete discrepancies:

1. `src/agent/loop.rs` still contains the complete synthetic `list .` bootstrap implementation behind `let bootstrap_allowed = false`. The code is unreachable but remains a large alternate execution path, including tool execution, artifact projection, context mutation, and retry control.
2. The primary loop still contains disabled narration and missing-structured-call branches expressed as `if false && ...`. This is dead implementation, not simplification.
3. After the bounded `AutonomyState::continuation_allowed()` branch, the primary loop still has a reachable repository-specific generic continuation branch that injects `"Continue working and use additional structured tool calls..."` without consulting the autonomy transition budget. That can create an additional provider turn outside the claimed single bounded owner.
4. The follow-up loop retains duplicate continuation/retry structure and dead `if false` branches. Its control flow is therefore not the same compact state-machine contract described by M005.
5. `ToolExecutionStatus` exists, but `tool_execution_status(rendered: &str)` currently infers denial/timeout/cancellation from rendered output substrings. A successful tool result containing words such as `permission`, `timeout`, or `cancel` can therefore be misclassified. `ToolExecutionOutcome` is present but is not the canonical production result boundary.
6. PR #74 correctly fixes an integration defect in broker principal binding, but its M009 closure record marks the workstream closed before the recovery discrepancies above are corrected. The closure evidence must therefore be reconciled after this pass rather than accepted as-is.

The original verification missed this because focused tests proved the new `AutonomyState` behavior and no-bootstrap observable behavior, but did not require deletion of unreachable legacy branches or prove that every reachable continuation passes through the state-machine budget. The hosted suite can pass while dead/duplicate control flow remains.

## 3. Explicit non-goals

Do not:

- redesign `RecoveryController`, goal persistence, provider retry, scheduler ownership, Tool Programs, or daemon lifecycle;
- add a new planner, workflow engine, recovery service, actor, task queue, or persistence schema;
- add new model-name heuristics or a replacement natural-language classifier;
- reintroduce generic synthetic repository bootstrap behavior;
- broaden tool authority, restore explicitly denied tools, or alter M001 permission semantics;
- weaken M002 textual-tool repair bounds or make repair available to structured-only profiles;
- change protocol/storage schemas;
- add CI lanes, matrices, scheduled checks, size gates, cargo-audit gates, coverage, benchmark gates, artifact publication, release automation, or a fixed release cadence;
- require a second full local workspace test when the existing hosted `verify` job provides the broad final-tree evidence;
- reopen M003/M004/M006/M007/M008 beyond focused integration regressions caused directly by this correction;
- refactor unrelated `AgentLoop` responsibilities merely because the file remains large.

## 4. Invariants that cannot regress

- All autonomous recovery/continuation transitions are bounded by one turn-local authority.
- No reachable branch may create an additional provider turn after the state-machine continuation budget is exhausted.
- Provider transport retry remains separate from model/autonomy recovery.
- Strong/structured-call model profiles never receive synthetic repository inspection.
- Textual tool-call repair remains adapter/profile-owned and receives at most its existing bounded allowance.
- A permission denial cannot trigger palette broadening or restoration of a denied/hidden tool.
- Cancellation/steering is observed before another autonomous provider/tool action.
- Workspace identity, snapshot ownership, broker principal binding, current-turn prompt selection, goal accounting, and terminal-event ownership from M001-M004 remain intact.
- Startup prompt compilation remains the M006 authority; this pass must not recreate recovery contracts in the stable startup prompt.
- Routine CI remains one bounded job and release remains manual.

## 5. Expected production-code changes

Inspect at minimum:

- `src/agent/loop.rs`;
- `src/agent/progress_recovery.rs`;
- the tool execution/broker result type feeding `AgentLoop::execute_tool_calls`;
- `tests/agent_loop_harness.rs`;
- focused `agent::progress_recovery` / loop tests;
- PR #74's broker-principal correction and subagent workspace fixture changes;
- `architecture/agent.md` and any recovery/tool execution documentation touched by M005/M009.

Expected changes are deliberately small:

1. Delete the unreachable synthetic bootstrap block rather than leaving it behind a constant false condition.
2. Delete dead narration/missing-call retry branches rather than retaining `if false` guards.
3. Remove the standalone repository-specific continuation branch, or route its intended one-turn behavior through the same `AutonomyState` transition method used by every other post-tool continuation.
4. Make the primary and follow-up loops share the same bounded continuation decision helper/state transition where practical; do not create a second state machine merely to deduplicate code.
5. Remove `bootstrap_used`/`mark_bootstrap_used` state if no supported profile retains a profile-specific bootstrap path. If a real supported profile still requires bootstrap, stop and document the concrete model evidence before retaining a profile-owned, at-most-once adapter path.
6. Carry typed execution status into recovery when the executor/broker already has that information. Keep model-facing rendered text for context, but do not use it as the authority for denial/timeout/cancellation classification when typed status is available.
7. Either make `ToolExecutionOutcome` the narrow recovery input or remove it if another existing result type already carries the required status. Do not add a duplicate wrapper that only mirrors another type.
8. Retain PR #74's correction binding `BrokerInvocationContext.principal_ref` to the same principal that issued the grant, not the decision/grant identity.

## 6. Target recovery contract

The final generic control flow should be explainable as:

```text
provider outcome
  -> structured/repaired tool calls: execute
  -> final answer with no pending continuation: finish
  -> malformed textual protocol: one adapter-owned repair allowance
  -> soft stop after tool work: one state-machine continuation/replan allowance
  -> explicit no-progress/repeat incident: RecoveryController transition
  -> transition budget exhausted or repeated no-progress: stall/finish with diagnostic
```

There must not be a second `is_repo_task_prompt(...)` or `indicates_more_work(...)` branch that can issue another provider request after the state machine has already consumed or denied its continuation allowance.

Natural-language heuristics may help choose the contents of the one allowed continuation message, but they must not create an additional independent continuation budget.

## 7. Typed tool-outcome boundary

Preferred shape:

```text
ToolExecutionOutcome {
    status: Success | Denied | Timeout | Cancelled | ToolError | ProtocolError,
    model_text: String,
    ...existing identifiers if already available
}
```

Requirements:

- use an existing typed executor/broker error/result classification if one already exists;
- do not parse `model_text` to override a known typed status;
- if legacy tools expose only rendered text, isolate the string classifier at that compatibility boundary and document it as fallback-only;
- a successful result containing the word `permission`, `timeout`, `cancel`, or `denied` must remain `Success` when execution reported success;
- permission denial remains distinguishable from generic tool failure so recovery cannot restore authority;
- do not change the model-facing text format merely to implement typed recovery state.

## 8. Ordered work packages

### Work package A — Reconcile the M009 candidate

1. start from the latest PR #74 candidate if still active; otherwise rebase equivalent M009 changes onto current `main`;
2. retain the broker-principal fix, workspace fixture corrections, project-catalog guard correction, and documentation fixes that are independently valid;
3. change M009 planning/closure status back to `closing` or `corrective pass required` until M010 closure evidence exists;
4. do not discard the already-green hosted candidate evidence; record it as predecessor evidence, not final M010 evidence.

### Work package B — Delete dead generic recovery code

1. remove the full `bootstrap_allowed = false` synthetic `list .` block;
2. remove `if false && ...` narration/missing-call branches from both primary and follow-up loops;
3. remove now-unused bootstrap state, helper functions, imports, counters, comments, and tests that only support unreachable behavior;
4. run compile/focused tests before further restructuring so deletion failures are localized.

### Work package C — Establish one reachable continuation authority

1. inventory every remaining `continue` after a provider response in the primary and follow-up loops;
2. identify which branches can create another provider turn;
3. ensure each autonomous provider-turn continuation is authorized by `AutonomyState` or is a distinct non-autonomy transport/cancellation path;
4. remove the standalone repository-specific generic continuation branch or fold it into the existing bounded continuation transition;
5. centralize the transition predicate/helper if doing so removes duplicated logic without obscuring control flow;
6. prove that after the allowed continuation is consumed, a soft-stop/repository heuristic cannot issue another autonomous provider request.

### Work package D — Complete typed outcome consumption

1. inspect the existing executor/broker return path for structured status information;
2. carry that status into recovery with the smallest compatible type change;
3. make `observe_tool_result` consume the typed status directly;
4. restrict rendered-string classification to legacy fallback-only inputs, if any remain;
5. remove unused `ToolExecutionOutcome` or use it as the real boundary; do not leave a dead architectural type.

### Work package E — Regression tests

Add deterministic tests proving at minimum:

- strong-model final answer performs one provider turn and no synthetic bootstrap;
- no `call_bootstrap_` path exists in production source/fixtures after deletion;
- post-tool soft stop receives at most one bounded continuation;
- a repository-analysis prompt cannot trigger a second continuation after the state-machine allowance is exhausted;
- primary and follow-up loops obey the same continuation bound;
- malformed textual tool protocol receives only the M002 adapter repair allowance;
- repeated equivalent failure reaches `Stall` within the configured bound;
- denied tool does not restore denied/hidden authority;
- typed `Denied`, `Timeout`, and `Cancelled` statuses reach recovery correctly;
- a successful tool result whose text contains `permission denied`, `timeout`, or `cancelled` remains `Success` when the execution status is success;
- cancellation/steering prevents another autonomous provider/tool action;
- PR #74 broker principal regression remains covered by the harness.

Do not add a regex/static guard that merely bans `if false` or `bootstrap_allowed`. Source deletion plus behavior tests are sufficient.

### Work package F — Documentation and strict closure reconciliation

1. update `architecture/agent.md` so the documented transition table matches the actual remaining branches;
2. update M005/M009 closure narrative only by adding corrective traceability; do not rewrite history to pretend the discrepancy was never present;
3. create `plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` with the final requirement-to-evidence matrix;
4. only after final hosted success, mark M010 and the subsystem closed and record M009 as reconciled by M010;
5. if a medium-or-higher recovery/authority defect remains, keep the subsystem open and create no further closure claim.

## 9. Concurrency, cancellation, restart, and failure semantics

Concurrency:

- parallel tool execution limits remain unchanged;
- this pass changes only whether/when another model turn is scheduled after observed output;
- do not reintroduce `ConstrainParallelism` unless a reachable, tested policy actually requires it.

Cancellation/steering:

- preserve the existing checks before provider/tool work;
- a pending continuation must be abandoned when cancellation/steering terminates or redirects the turn;
- no deleted branch may be replaced by an uninterruptible helper loop.

Restart:

- recovery state remains turn-local and non-durable;
- no storage migration or restart replay mechanism is required.

Failure:

- malformed protocol, denied execution, timeout, cancellation, tool error, and provider transport error remain distinguishable;
- provider transport retry retains its own bounded retry mechanism and does not consume autonomy transitions unless a semantic provider outcome is produced.

## 10. Security and authorization review

This corrective pass is security-sensitive because recovery control flow sits immediately after permissioned tool execution.

Verify explicitly:

- PR #74's broker principal fix is retained: the broker context principal matches the grant issuer principal;
- a permission decision/grant ID is never substituted for principal identity;
- denial cannot trigger base-palette restoration that reintroduces a denied tool;
- textual repair still routes through the ordinary permission/broker path;
- no recovery branch fabricates new permission metadata or bypasses `Ask`;
- removing dead bootstrap code also removes a dormant path capable of executing `list` without a model-issued structured call.

## 11. Storage, protocol, migration, compatibility, and user-visible effects

Storage/protocol:

- no changes expected.

Compatibility:

- structured-call providers should observe fewer accidental extra turns, not fewer supported capabilities;
- fragile models retain the M002 adapter repair path;
- if no real profile requires synthetic bootstrap, deleting dead bootstrap code is behavior-preserving because the branch is currently unreachable;
- a provider/model that depended on the unbudgeted repository continuation is relying on behavior that contradicts M005's accepted bounded contract; do not preserve it without explicit model evidence.

User-visible behavior:

- fewer duplicate/narration continuation turns;
- more predictable stop/stall behavior;
- no change to tool names, permission prompts, project/session protocol, or release workflow.

## 12. Verification posture

Keep verification minimal and targeted.

Required focused checks should include the narrow equivalents of:

```bash
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
cargo test --test subagent -- --test-threads=1   # only if retained M009 fixture edits are touched
scripts/verify.sh quick
```

Run the provider parser tests only if M002 repair plumbing is touched.

Run `git diff --check` and the existing relevant security/authority guards already included by `scripts/verify.sh quick`.

Final broad evidence:

- one ordinary existing hosted `CI / verify` run on the exact final corrective merge candidate.

Do not add or require:

- a second CI lane;
- a feature matrix;
- a local full-workspace run in addition to the hosted final run unless a focused failure makes it necessary;
- cargo-audit/size/coverage/benchmark gates;
- release automation.

## 13. Acceptance criteria

M010 closes only when all of the following are true:

- the synthetic bootstrap implementation is deleted from the generic loop, not merely disabled by a constant false condition;
- dead `if false` narration/missing-tool recovery branches are removed from primary and follow-up loops;
- no standalone repository-specific or natural-language heuristic can issue an autonomous provider turn outside `AutonomyState`'s bounded transition authority;
- primary and follow-up execution paths use one coherent continuation budget/decision contract;
- one post-tool continuation allowance cannot be followed by a second generic repository continuation;
- textual repair remains the sole M002 adapter-owned protocol repair path;
- permission denial cannot broaden/restore denied authority;
- typed execution status is consumed wherever the executor/broker already knows it;
- rendered-string status classification is removed from authoritative typed paths and, if retained for legacy compatibility, is isolated and tested as fallback-only;
- successful rendered output containing denial/timeout/cancellation words cannot be misclassified when execution reported success;
- unused recovery/bootstrap state and dead architectural wrappers are removed rather than retained speculatively;
- PR #74's broker-principal and explicit-workspace fixture corrections are retained;
- focused recovery/loop/harness tests pass;
- `scripts/verify.sh quick` passes;
- the existing hosted `verify` job passes on the exact final corrective candidate;
- `010-status.md` classifies unresolved findings and contains no critical/high/medium issue in this scope;
- M009 is reconciled as predecessor integration evidence rather than the final strict closure authority;
- `plans/registry.md` and the subsystem roadmap mark the workstream closed only after M010 evidence is accepted.

## 14. Stop conditions

Stop and report rather than broadening this pass if:

- a supported model can be proven to require a generic synthetic bootstrap or a second independent repository continuation;
- carrying typed execution status requires a public Tool trait/protocol rewrite rather than a narrow internal result boundary;
- removing the repository continuation reveals a separate goal-continuation product requirement that belongs in the durable goal subsystem;
- PR #74 has diverged with unrelated product work that cannot be cleanly rebased.

In those cases, preserve the bounded M005 authority model and split only the newly proven requirement. Do not restore overlapping generic recovery behavior as a compatibility shortcut.

## 15. Required closure evidence

`plans/closure/agent-runtime-correctness-autonomy-simplification/010-status.md` must include:

- final implementation commit/PR and exact hosted `verify` run ID;
- predecessor PR #74/M009 disposition;
- before/after inventory of every provider-turn-producing recovery/continuation branch in primary and follow-up loops;
- evidence that synthetic bootstrap and disabled legacy branches were physically deleted;
- final `AutonomyState` transition/budget table;
- typed tool-outcome/status data-flow description and any fallback string classifier that remains;
- tests proving repository heuristics cannot bypass the continuation budget;
- permission/authority regression evidence including broker principal consistency;
- focused test and `scripts/verify.sh quick` results;
- documentation reconciliation list;
- unresolved findings by severity;
- final recommendation: closed, conditionally closed, corrective pass required, or blocked.

Strict closure requires `closed`. A green build alone is not sufficient if any acceptance criterion above remains false.
