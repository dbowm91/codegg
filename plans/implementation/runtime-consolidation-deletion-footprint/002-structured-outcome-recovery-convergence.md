# Runtime Consolidation, Deletion, and Footprint M002 — Structured Outcome and Recovery Convergence

Status: ready

Source roadmap:

- `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`

Relevant references:

- `plans/000-long-term-specification.md` sections 2, 4.2, 4.7, and 8.7
- `architecture/agent.md`
- `architecture/tool.md`
- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- closed predecessor work under `plans/implementation/agent-runtime-correctness-autonomy-simplification/`

Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`

Primary class: correctness invariant / simplification

Dependencies:

- hard: none; preserve the M011 typed-outcome fixes already landed;
- interface: existing Tool Broker/ToolContract and `ToolExecutionOutcome` semantics;
- downstream: M003 is hard-blocked until this milestone closes.

Target closure record:

- `plans/closure/runtime-consolidation-deletion-footprint/002-status.md`

## 1. Objective

Finish the transition from text-derived recovery heuristics to one structured execution/recovery contract. Correct the known result-equivalence defect, make already-known execution status authoritative, define bounded observable progress/effect facts, and collapse recovery states that do not have distinct runtime semantics.

This milestone is deliberately narrower than an autonomy redesign. The target is a smaller and more truthful state machine.

## 2. Current implementation evidence

Inspect at minimum:

- `src/agent/progress_recovery.rs`;
- all `AgentLoop` sites constructing or consuming `ToolExecutionOutcome`;
- `src/tool/broker.rs`, `src/tool/contract.rs`, and structured-result types;
- MCP/question timeout and cancellation branches;
- subagent/task result propagation;
- focused recovery and loop harness tests.

Known baseline findings:

- `ToolExecutionStatus` distinguishes success, denied, timeout, cancelled, tool error, and protocol error.
- `observe_tool_result()` currently gives special recovery semantics primarily to `Denied`; other typed statuses can still rely on text-derived `error_class` behavior.
- `classify_error()` still provides a legacy prose parser.
- `RecoveryController::detect()` can report `EquivalentResult` when the current result fingerprint matches any history record, rather than a record constrained to the same action identity.
- `new_evidence`, `state_changed`, and `child_advanced` collapse to `ProgressSignal::StateChanged`; `DifferentTool` is declared but not emitted by this path.
- changed result fingerprints can count as progress, which is useful for read-only evidence gathering but unsafe as the sole progress definition for volatile/nondeterministic tools.

## 3. Explicit non-goals

Do not:

- increase recovery budgets or add more retry stages;
- add an LLM-based progress judge;
- add repository-specific heuristics;
- infer failure status from arbitrary successful prose when typed execution status exists;
- make non-idempotent mutations transparently retryable;
- redesign provider retry/backoff;
- redesign goals/todos/subagent scheduling;
- make recovery persistence durable across daemon restart;
- add a new generic workflow/state-machine framework.

## 4. Invariants that cannot regress

- structured provider tool calls remain the canonical execution signal;
- textual compatibility repair remains adapter-owned and bounded;
- provider transport retries remain separate from semantic recovery;
- permission denial cannot broaden tool authority or restore a palette in a way that defeats the denial;
- child authority cannot exceed parent authority;
- model-facing result text remains available and compatible while internal status/effect facts become authoritative;
- progress history remains bounded and contains no raw tool arguments, full outputs, or private model reasoning;
- recovery remains deterministically bounded.

## 5. Target internal contract

Use the smallest existing type extension that can represent:

- terminal execution status;
- model-facing text;
- whether durable/observable state changed;
- whether new evidence was produced;
- whether a child job/run advanced;
- optional stable effect/result identity suitable for repeat detection;
- contract-derived retry/idempotency information only where already available.

Do not create parallel copies of Tool Broker provenance/contract metadata. Recovery should consume references or normalized facts derived from canonical broker results.

A suggested internal normalization is:

```text
ExecutionObservation
  status
  action_identity
  result_identity
  effect: none | evidence | state_change | child_advance
  retry_class/idempotency (optional, derived)
```

Exact naming is implementation-dependent.

## 6. Ordered work packages

### A. Fix equivalence and identity semantics

1. Define the intended identity for an equivalent repeated action: canonical tool plus normalized argument fingerprint and, where relevant, selected tool-surface/contract revision.
2. Restrict `EquivalentResult` comparison to the intended action identity.
3. Distinguish exact repeated result from same action producing a changed result.
4. Add regression tests proving an unrelated earlier tool with the same textual result cannot cause `EquivalentResult` for the current action.
5. Preserve bounded normalized fingerprints; do not store raw arguments/results.

### B. Make typed status authoritative

1. Enumerate every production path constructing a tool outcome.
2. Ensure explicit timeout branches yield `Timeout`, explicit cancellation yields `Cancelled`, permission rejection yields `Denied`, malformed/provider/tool-protocol errors yield the documented non-success status, and ordinary broker errors remain `ToolError` unless stronger typed information exists.
3. Recovery must branch on typed status before any legacy text classifier.
4. `classify_error()` may remain only at named opaque compatibility seams that genuinely receive text without status.
5. If no valid seam remains, delete the legacy classifier instead of preserving it speculatively.

### C. Define meaningful progress

1. Make state/effect facts the primary progress signal for mutating, child, and structured evidence-producing tools.
2. Permit changed result identity as progress only for read-only/evidence-gathering actions where novelty is meaningful and bounded.
3. Ensure volatile output alone cannot reset the recovery budget indefinitely for a repeatedly useless command/tool.
4. Derive child progress from an actual child run/job state/version transition where the current architecture exposes one; do not fabricate a boolean from text.
5. Avoid semantic analysis of arbitrary output prose.

### D. Collapse unused semantic states

Review `ProgressSignal`, `IncidentKind`, `RecoveryAction`, `AutonomyPhase`, and overlapping counters.

For each state/variant:

- retain it only if runtime code emits it and some behavior/observability meaning differs;
- otherwise remove/merge it and update tests/docs;
- keep adapter repair distinguishable from semantic recovery, but avoid duplicate recovery budgets/owners.

The desired result is one per-turn autonomy/recovery owner with one bounded semantic recovery budget. Provider transport retry remains outside it.

### E. Contract-aware retry boundary

Where `ToolContract` already exposes retry/idempotency/effect class:

- allow recovery to know whether a retry is safe;
- never silently retry a non-idempotent mutation solely because output/error changed;
- do not add new retry machinery if the ordinary loop currently asks the model to choose the next call; simply prevent recovery policy from recommending unsafe automatic replay.

### F. Tests and documentation

Add deterministic tests proving at minimum:

- unrelated identical result text does not produce `EquivalentResult`;
- repeated same action/same result does;
- same read-only action producing genuinely new evidence may count as progress;
- repeated mutating/no-effect or volatile-output actions do not reset recovery indefinitely;
- typed `Denied`, `Timeout`, `Cancelled`, `ProtocolError`, and `ToolError` remain distinct through recovery where those statuses are known;
- a successful result containing failure-like words remains success;
- denial cannot trigger authority broadening;
- recovery/stall budgets remain bounded;
- child progress is based on a real child transition where available.

Update `architecture/agent.md` and `architecture/tool.md` to describe structured outcome/effect ownership, not the transitional string classifier.

## 7. Concurrency, cancellation, restart, failure semantics

Concurrency:

- parallel tool-batch ordering and existing semaphores remain unchanged;
- observations must retain original tool-call association under parallel completion.

Cancellation:

- explicit cancellation is terminal for that execution observation;
- cancellation is not inferred from model/tool prose.

Restart:

- recovery state remains turn-local unless an existing durable run primitive already records a fact needed for child progress;
- no new schema.

Failure:

- failure status and model-visible text are separate concerns;
- a failed tool may still produce bounded diagnostic evidence, but it does not become success/progress by string novelty alone.

## 8. Verification

Expected focused verification:

```bash
cargo test -p codegg --lib agent::progress_recovery -- --nocapture
cargo test -p codegg --lib agent::r#loop::tests -- --nocapture
cargo test --test agent_loop_harness -- --test-threads=1
scripts/verify.sh quick
git diff --check
```

Add Tool Broker or task/subagent focused tests only when those production types are modified.

Because M003 depends on this contract, run workspace Clippy before closure if public/private type moves touch broad call sites:

```bash
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
```

## 9. Explicit acceptance criteria

M002 is complete only when:

1. `EquivalentResult` is scoped to the correct action identity and cannot be triggered by an unrelated historical result.
2. Known typed status is consumed directly by recovery; text classification is absent from ordinary typed paths.
3. Any remaining legacy text-classification seam is named, justified, tested, and documented; otherwise the classifier is deleted.
4. Successful output containing words such as `permission denied`, `timeout`, or `cancelled` remains `Success` end to end.
5. Explicit timeout/cancellation/denial branches remain distinguishable through recovery.
6. Mutating or side-effecting actions require real effect/state progress rather than mere output novelty to reset semantic recovery.
7. Read-only/evidence novelty may count as progress only under a bounded documented rule.
8. Child progress derives from an actual child state transition where supported.
9. Recovery/autonomy variants and counters with no distinct semantics are removed or merged.
10. Semantic recovery remains bounded and provider retry remains separately owned.
11. No hidden retry of non-idempotent mutation is introduced.
12. Focused recovery/loop tests and `scripts/verify.sh quick` pass.
13. Architecture docs describe the final structured status/effect boundary.
14. M003 has a stable written internal contract to target.

## 10. Stop conditions

Stop and record a blocker if completing structured recovery would require:

- a public Tool Broker protocol change;
- a durable schema migration;
- a new generalized state-machine framework;
- unbounded semantic inspection of tool/model text;
- broad Tool Program redesign.

In that case, close only the independently correct defect fixes and write a bounded follow-up rather than expanding M002.
