# Retry Ownership

One bounded retry chain spans provider, tool, scheduler, and compatible
external-effect paths (M002). Lower layers consume but never replenish the
parent budget. Ambiguous commits become typed uncertainty, never blind
replay.

## Chain contract (`codegg-providers/src/retry.rs`)

```rust
pub struct RetryContext { chain_id, total_attempts, attempts_remaining, deadline }
pub enum AckState { NotDispatched, FailedBeforeCommit, Acknowledged, Uncertain }
pub struct UncertainSideEffect { chain_id, domain, operation, invocation_key, detail }
pub enum ReconciliationOutcome { Reconciled, Unreconciled(Uncertain), NotApplicable }
pub enum UnifiedRetryDisposition { Never, Transient, RateLimited, AuthRefreshable, UncertainSideEffect }
```

- Hard ceilings: `MAX_CHAIN_ATTEMPTS = 8`, `MAX_CHAIN_DURATION = 300s`.
  Provider (3) and tool (`1 + max_retries`) caps are lower ceilings.
- Created at the logical operation/turn boundary
  (`RetryContext::for_operation()` = 6 attempts / 180s;
  `for_provider_turn()` = 3 / 120s; `single_attempt()` for legacy).
- `derive_child(max)` shares the chain ID and deadline but caps remaining;
  `consume_one()` accounts every failed attempt, including the terminal one.
- `RetryContextDto` carries `remaining_millis`, never `Instant`, across
  durable/protocol boundaries. `from_dto` never restores more than carried.
- `UncertainSideEffect.detail` is truncated to 500 chars and must already
  be secret-safe; diagnostics carry chain/domain/operation/key only, never
  arguments, commands, URLs, or credentials.

## Provider (`src/agent/provider_turn.rs`)

- `receive_with_retry_context(loop_, request, Option<RetryContext>)`
  binds to the caller chain; `receive` preserves legacy single-chain
  behavior via `for_provider_turn()`.
- Effective attempts = `min(3, chain.remaining)`; each failure consumes one;
  deadline expiry stops with the last typed error.
- M001 visible-output gate unchanged: replay after visible output is still
  supersession, not retry. Chain ID appears in tracing alongside attempt ID.
- `AgentLoop` (`loop.rs`, `follow_up.rs`) creates one `for_operation()`
  chain per turn and passes it to the provider; tool execution derives
  narrower children from the same chain.

## Tools (`src/tool/retry.rs`, `broker.rs`, `contract.rs`)

- `ToolTerminalStatus::UncertainSideEffect` + `ToolValue::uncertain()` is the
  typed ambiguous-commit result. `ProgrammaticOutcome::UncertainSideEffect`
  keeps it out of `CompletedCall`; only `Success` completes.
- `decide_tool_retry(contract, ack, key, error, chain)`:
  - Denied/disabled/not-found/format → `DoNotRetry` (Never), no budget burn
    as transient.
  - Non-retryable execution → `DoNotRetry`.
  - Expired/exhausted chain → `DoNotRetry`.
  - Uncertain + `NonIdempotent`/`ProcessExec` → `Uncertain` even when the
    contract disables retry (uncertainty is surfacing, not replay).
  - Uncertain + `IdempotentMutating` without a stable key → `Uncertain`.
  - `ReadOnly`/`ReadValidate`/`SafeRepeat` transient → `Retry`.
  - `IdempotentMutating` transient with a stable key → `Retry` (same key).
  - `NonIdempotent`/`ProcessExec` before dispatch → bounded `Retry`;
    after dispatch → `Uncertain`.
- `ack_for_dispatch_phase(dispatched, error)`: pre-invocation → 
  `NotDispatched`; `Timeout`/`Network`/`Io` after dispatch → `Uncertain`;
  explicit rejections → `Acknowledged`.
- `ToolBroker::execute` preserves legacy single-attempt semantics.
  `execute_with_retry(registry, tool, input, ctx, Option<RetryContext>)`
  validates authority once, reuses the same context (no new permissions,
  no sandbox/model/provider change), keeps a stable invocation key,
  backoffs from the contract policy with cancellation-aware sleep, and
  emits chain/disposition tracing without payloads.
- `validate_retry_contract` rejects blind retry on
  `NonIdempotent`/`ProcessExec` and keyless `IdempotentMutating` at
  registration time. Legacy `ToolContract::legacy()` stays
  `NonIdempotent` / `max_retries=0` (non-retryable).

## Reconciliation (narrow, per-domain)

- Scheduler: `JobSubmissionService::reconcile_by_key` checks the in-memory
  fast path then the durable store scan; fingerprint mismatch is
  `SubmissionKeyConflict`, never a duplicate. `submit` already converges;
  `reconcile_by_key` is the explicit lost-ack path. Durable jobs retain
  canonical attempt semantics.
- Git: `reconcile_git_operation` returns `NotApplicable` for reads and
  pre-dispatch failures; `Unreconciled(git:push…)` for uncertain mutations.
  The Git tool must inspect the exact remote/ref via existing read paths
  before any retry.
- External APIs: `reconcile_idempotent_api` returns `Reconciled` only with
  both a stable key and proven backend dedup/status support; otherwise
  `Unreconciled` (never invents idempotency).

## Observability

Tracing carries `chain_id`, `attempt_index`, `error_class`,
`attempts_left/consumed`, and `uncertain.summary()` (domain/operation/chain/
key). No arguments, commands, or secrets. Bus events unchanged (M001
attempt lifecycle); retry-chain diagnostics are tracing-local to avoid
breaking protocol compat.

## Qualification (M008)

Integrated fault-injection qualification lives in
`tests/reliability_qualification_m008.rs`: transient/permanent/conditional
taxonomy, bounded `Retry-After`, nested-budget exhaustion, cancellation
during backoff, idempotent recovery vs exactly-once uncertain surfacing,
and secret-safe diagnostics. No production change; the suite proves the
M001+M002 contract holds under injected faults.

## What is out of scope

No semantic rollback of arbitrary shell, no distributed transactions, no
invented API idempotency, no scheduler-attempt ownership change, no
approval/sandbox or provider-failover change.
