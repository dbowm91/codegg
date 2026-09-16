# Execution Reliability, Approval, and Autonomy M002 — Unified Retry Budget and Side-Effect Reconciliation

Status: blocked

Repository baseline: `18365458f881f6ac4524c9ea05224b69923faa4f`

Source roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Long-term requirements:

- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`

Primary class: invariant / infrastructure

Hard dependency: M001 closure.

## 1. Objective

Prevent multiplicative nested retries and unsafe replay by introducing one bounded retry chain/deadline propagated through provider, tool, scheduler/job, and compatible external-effect paths. When an operation may have committed but acknowledgement was lost, classify it as an uncertain side effect and reconcile canonical external/local state before any retry.

## 2. Why this milestone is blocked

M001 must first establish provider attempt identity and stable transient/permanent taxonomy. M002 then composes that with existing ToolContract/job semantics rather than competing with provider-local retry logic.

## 3. Current implementation evidence

- provider turn retries, fallback backoff/circuit breakers, ToolRetryPolicy, scheduler Job/Attempt retries, process execution and adapter-specific retry behavior exist in separate layers;
- `ToolEffectClass` identifies ReadOnly, ReadValidate, SafeRepeat, IdempotentMutating, NonIdempotent and ProcessExec, with `is_retry_eligible()`;
- `IdempotencyClass` and invocation keys already exist for tool/program execution;
- scheduler/run stores provide durable attempt identity for heavy work;
- no one retry context currently bounds the complete nested chain;
- acknowledgement loss after an external mutation (push/API/MCP/etc.) is not equivalent to a known failure-before-dispatch.

## 4. Invariants that must not regress

- non-idempotent action with unknown acknowledgement is never automatically replayed merely because a timeout/connection reset occurred;
- lower layers may consume but not reset/increase the parent retry budget;
- one retry chain has a hard attempt and elapsed-time bound;
- scheduler-owned durable jobs retain their canonical attempt/reconciliation semantics;
- read-only/idempotent operations remain recoverable without needless user intervention;
- invocation/idempotency keys remain stable across safe retries where the backend supports them;
- retry state cannot grant new permissions or alter sandbox/model/provider selection.

## 5. Scope

### In scope

- `RetryContext`/chain ID/deadline/remaining-attempt contract;
- propagated retry disposition/consumption semantics;
- effect/idempotency-aware tool retry adapter;
- acknowledgement state (`not_dispatched`, `failed_before_commit`, `acknowledged`, `uncertain`) or equivalent;
- reconciliation hooks for known canonical domains (Git, scheduler jobs, idempotent external APIs where available);
- typed `UncertainSideEffect` result when safe reconciliation is unavailable;
- metrics/events/tests.

### Explicitly out of scope

- automatic semantic rollback of arbitrary shell commands;
- a distributed transaction system;
- inventing idempotency support for APIs that do not offer it;
- changing scheduler retry ownership;
- approval/sandbox implementation;
- broad provider failover.

## 6. Required production changes

### Core/domain

Define a small retry contract available below AgentLoop, for example:

```rust
struct RetryContext {
    chain_id: RetryChainId,
    deadline: InstantOrDeadline,
    attempts_remaining: u8,
    total_attempts: u8,
}

enum RetryDisposition {
    Never,
    Transient,
    RateLimited { retry_after: Option<Duration> },
    AuthRefreshable,
    UncertainSideEffect,
}
```

Exact ownership may live in core/provider/tool shared modules, but there must be one semantic definition. Avoid exposing process-local `Instant` in durable/protocol DTOs; transport a duration/deadline representation appropriate to each boundary.

### Provider integration

M001 provider attempts consume the caller's RetryContext rather than creating an independent unbounded local budget. Existing provider-specific caps remain lower ceilings.

### Tool integration

Map ToolContract effect/idempotency and execution phase to retry eligibility:

- ReadOnly/ReadValidate/SafeRepeat: safe bounded retry for transient execution failures;
- IdempotentMutating: retry only with stable invocation key and backend semantics known to overwrite/deduplicate safely;
- NonIdempotent: no automatic retry after dispatch unless backend idempotency key/reconciliation proves safety;
- ProcessExec: classify command-specific/managed-job semantics; raw shell is conservative.

Legacy tools with conservative default contracts remain non-retryable.

### External side-effect reconciliation

Introduce narrow per-domain reconciliation, not a generic guesser. Examples:

- scheduler: query durable job/attempt by submission/invocation ID;
- Git push/remote mutation: inspect exact remote/ref expected result before retry if existing Git service safely supports it;
- API/MCP mutation: use provider/tool idempotency key or explicit get/status operation if defined by contract;
- otherwise: return `UncertainSideEffect` and require model/user recovery instead of replay.

### Runtime/concurrency

RetryContext is created at the logical operation/turn boundary and passed downward. Nested operations cannot replenish it. Cancellation/deadline stops all further attempts.

### Protocol/frontends

Expose bounded reason/chain/uncertain-effect diagnostics where useful. Do not expose internal stack or sensitive arguments.

### Documentation/static guards

Document canonical retry ownership and contract. Add source guard only if multiple new retry loops could bypass the context; otherwise use focused tests/code review.

## 7. Ordered work packages

### Work package A — RetryContext contract

Implement chain/deadline/attempt accounting and propagation through M001 provider path and ToolBroker/compatible scheduler submission seams.

### Work package B — Tool effect mapping

Make ToolContract retry policy consume the shared budget and reject unsafe combinations at validation/runtime.

### Work package C — Uncertain-side-effect state

Represent dispatch/ack ambiguity and add narrow reconciliation hooks for existing durable/idempotent domains.

### Work package D — Observability/bounds

Record chain ID, attempts consumed, final disposition and reconciliation result without sensitive payloads.

## 8. Failure, cancellation, restart, and contention semantics

- budget/deadline exhaustion returns the last meaningful typed failure; no hidden extra retry layer continues;
- cancellation wins over backoff/reconciliation;
- restart does not regenerate a process-local retry chain for an ambiguous side effect and replay it; durable job/external identity is reconciled first;
- concurrent duplicate submissions with the same idempotency key converge on one canonical durable result when supported;
- failure to reconcile an uncertain effect remains uncertain, never converted to failed/safe-to-retry by absence of evidence.

## 9. Compatibility and migration

Existing tool/provider APIs may receive optional RetryContext with conservative default when absent. Legacy callers keep current single-attempt semantics where necessary. No database migration is required unless durable uncertain-effect metadata is attached to existing Run/Attempt records; if so, use additive nullable fields.

## 10. Required tests

### Focused unit tests

- budget/deadline consumption;
- nested context cannot increase remaining attempts;
- ToolEffectClass/idempotency matrix;
- acknowledgement-state transitions;
- reconciliation result mapping.

### Integration tests

- provider transient retry + tool retry share bounded total chain;
- idempotent write retries with same key;
- non-idempotent dispatched timeout returns UncertainSideEffect;
- durable scheduler submission reconciles existing job rather than duplicating;
- raw shell ambiguous execution is not automatically replayed.

### Restart and recovery tests

- restart after durable job submission before acknowledgement;
- restart with uncertain external mutation retains reconciliation requirement.

### Contention and cancellation tests

- concurrent duplicate idempotent submissions;
- cancel during backoff/reconciliation.

### Security and negative tests

- permission denied is Never retry;
- retry cannot broaden sandbox/authority;
- diagnostics omit command/API secrets.

### Migration and compatibility tests

As required by any additive run/job fields.

## 11. Required verification commands

```bash
cargo test -p codegg-providers
cargo test --test tool_structured_execution
cargo test --test command_routing_execution_ownership
cargo test --test scheduler_contention
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

## 12. Documentation updates

- provider retry architecture;
- `architecture/tool.md` retry/effect contract;
- scheduler/jobs/run-store docs as touched;
- new retry ownership doc if clearer than scattering the contract.

## 13. Acceptance criteria

- one logical retry chain bounds nested layers;
- idempotent/read operations recover within policy;
- ambiguous non-idempotent effect is reconciled or surfaced, never blindly replayed;
- durable job submission does not duplicate after lost acknowledgement;
- cancellation/deadline stops retries promptly;
- current permission/sandbox/provider-selection authority is unchanged.

## 14. Stop conditions

Stop if implementation requires a generic distributed transaction manager, parsing arbitrary shell side effects to prove idempotency, or changing scheduler durable-attempt ownership.

## 15. Closure evidence required

- retry-chain propagation diagram and attempt-count tests;
- effect/idempotency matrix;
- uncertain-effect/reconciliation scenarios;
- restart/duplicate-submission evidence;
- verification commands and residual unsupported reconciliation domains.

## 16. Handoff notes

Be conservative on `ProcessExec` and unknown external tools. Returning an explicit uncertain result is preferable to “helpful” replay that may push, deploy, delete, purchase, or notify twice.
