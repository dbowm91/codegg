# Agent Runtime Correctness — Goal Verification Corrective Addendum

Status: active — M013 ready

Repository baseline reviewed: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Parent roadmap/addendum:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`
- `plans/subsystems/agent-runtime-goal-verification-addendum.md`

Historical milestone preserved:

- M012 closed: `plans/closure/agent-runtime-correctness-autonomy-simplification/012-status.md`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## 1. Corrective purpose

M012 established the essential authority boundary: the working model proposes completion, a host-owned deterministic verifier evaluates durable evidence, and only a revision-checked host transition may mark the goal complete. A post-closure audit found that positive evidence correlation and natural-language criterion interpretation remain weaker than the original plan required.

This addendum preserves M012 as the historical removal of model self-certification and adds one corrective milestone. It does not introduce an LLM verifier, new scheduler, or new workflow engine.

## 2. Discovered corrective findings

### Goal provenance is inferred too broadly

Current evidence assembly accepts Test/Subagent jobs from the same session created after the goal began. That is host-owned state but not exact goal ownership. Unrelated later jobs in a long-lived session can therefore become positive or negative evidence for the active goal.

The original plan called for host-written active-goal provenance on jobs/runs. M013 must complete that relation using existing durable job metadata where safe, or a narrow typed field only if reserved labels cannot be made unambiguous.

### Positive test claims can borrow unrelated passing evidence

A proposal's `tests_run` strings are not matched to an exact host test. The verifier currently proves at most that some eligible test passed. After exact goal provenance this is substantially safer, but the contract must explicitly distinguish `the goal has a passing supervised test` from `the specific model-named test X ran` unless durable invocation identity supports the latter.

### Natural-language criteria are classified by substrings

The deterministic verifier currently uses words such as `test`, `pass`, `green`, `todo`, and `task` to infer criterion semantics. This can misclassify phrases such as `Pass security review` as test-verifiable.

A deterministic verifier must only claim semantic proof for explicit typed/canonical criteria it actually owns. Unsupported natural-language criteria remain unresolved/`AwaitingUser` rather than being guessed.

### File claims remain explanatory

Model `files_changed` claims are bounded but not host-derived. They currently do not independently elevate completion, which is safe. M013 must preserve that property and use existing Git/checkpoint evidence only where exact changed-file proof is required and cheaply available.

## 3. Invariants

- direct model-owned completion remains impossible;
- only a host `Met` verdict can complete an active goal;
- relevant failed/in-flight goal-owned evidence overrides model prose;
- positive evidence is related to the exact goal by host-written provenance;
- same-session activity from another goal cannot satisfy or poison the active goal;
- unsupported natural-language criteria cannot become `Met` through keyword heuristics;
- model test/file strings remain claims unless exact host evidence supports them;
- verification stays read-only and plugin-independent;
- existing continuation, budget, pause, cancel, replacement, and steering authority remains unchanged;
- missing legacy provenance fails conservatively.

## 4. Corrective milestone

### M013 — Goal evidence provenance and criterion corrective pass

Status: ready

Plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/013-goal-evidence-provenance-and-criterion-corrective-pass.md`

Class: corrective invariant / provenance / goal-state correctness

Dependencies:

- hard: none beyond historical M012 implementation on `main`;
- interface: existing durable GoalStore, JobRecord labels/store, supervised test/subagent submission paths, and TodoStore.

Exit conditions:

- supervised Test/Subagent jobs that may count toward completion carry host-written exact goal provenance;
- evidence assembly filters by exact goal provenance, with session/time only as secondary bounds;
- unrelated jobs in the same session cannot satisfy or block the active goal;
- loose substring criterion inference is removed;
- unsupported natural-language criteria become unresolved/`AwaitingUser` rather than guessed;
- model-named tests/files cannot create positive proof without appropriate host evidence;
- direct model completion, CAS protection, and existing continuation semantics remain intact;
- focused tests, Clippy, and `scripts/verify.sh quick` pass;
- closure record is written at `plans/closure/agent-runtime-correctness-autonomy-simplification/013-status.md`.

## 5. Why the corrective milestone is independently ready

The repository already has:

- durable goal IDs and revisions;
- durable `JobRecord.labels` suitable for bounded host metadata if reserved ownership can be enforced;
- supervised test/subagent job creation seams;
- stateless deterministic verification;
- restart-safe Job/Todo stores;
- adversarial goal completion tests from M012.

No new execution subsystem or semantic model is required.

## 6. Verification posture

Use focused goal core/tool tests, same-session multi-goal provenance fixtures, restart reconstruction, adversarial natural-language criteria, targeted Clippy, and `scripts/verify.sh quick`.

Do not add an LLM evaluator suite, new CI lane, workflow engine, or broad job-schema migration unless reserved labels prove structurally unsafe.

## 7. Deferred work remains deferred

- optional read-only semantic/LLM criterion adjudication;
- rich criterion visualization;
- plugin-defined completion authority;
- cross-session autonomous goal scheduling;
- a new file-history system solely for changed-file verification.

## 8. Closure disposition

M012 remains valid historical evidence that direct model self-certification was removed. Until M013 closes, the goal-verification line should be considered active corrective work because exact evidence ownership and criterion semantics are not yet strict enough for final closure.
