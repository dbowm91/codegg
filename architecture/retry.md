# Retry Ownership

One bounded retry chain spans provider, tool, scheduler, and compatible
external-effect paths (M002). Lower layers consume but never replenish the
parent budget. Ambiguous commits become typed uncertainty, never blind
replay.

## Purpose

Define **one** bounded retry budget for a logical operation and make every
layer below it a *consumer* of that budget rather than an independent retry
loop. Where a failure leaves the commit state unknown, the contract says so
explicitly (`UncertainSideEffect`) instead of replaying a possibly-committed
mutation.

## Where It Lives

| Artifact | Location |
|----------|----------|
| Chain contract (types, DTO, ceilings) | `crates/codegg-providers/src/retry.rs` |
| Tool-level decision matrix + reconcilers | `src/tool/retry.rs` |
| Tool contract (effect class, retry policy) | `src/tool/contract.rs` |
| Broker retry loop | `src/tool/broker.rs` (`ToolBroker::execute_with_retry`) |
| Provider-turn retry loop | `src/agent/provider_turn.rs` |
| Durable-job submission reconciliation | `src/scheduler/submission.rs` (`reconcile_by_key`) |
| Qualification suites | `tests/agent_loop/reliability_qualification_m008.inc`, `tests/agent_loop/retry_budget_reconciliation.inc` |

The chain contract is exported from the providers crate
(`codegg_providers::retry`) and re-exported by the root crate as
`codegg::provider::retry`; `src/tool/retry.rs` imports it from there.

## How It Works

### Chain contract (`codegg-providers/src/retry.rs`)

```rust
pub struct RetryContext { chain_id, total_attempts, attempts_remaining, deadline, created_at }
pub enum AckState { NotDispatched, FailedBeforeCommit, Acknowledged, Uncertain }
pub struct UncertainSideEffect { chain_id, domain, operation, invocation_key, detail }
pub enum ReconciliationOutcome { Reconciled { detail }, Unreconciled(UncertainSideEffect), NotApplicable }
pub enum UnifiedRetryDisposition { Never, Transient, RateLimited { retry_after }, AuthRefreshable, UncertainSideEffect }
```

- Hard ceilings: `MAX_CHAIN_ATTEMPTS = 8`, `MAX_CHAIN_DURATION = 300s`.
  Provider (3) and tool (`1 + max_retries`) caps are lower ceilings.
- Created at the logical operation/turn boundary
  (`RetryContext::for_operation()` = 6 attempts / 180s;
  `for_provider_turn()` = 3 / 120s; `single_attempt()` = 1 / 60s for legacy).
- `derive_child(max)` shares the chain ID and deadline but caps remaining;
  `consume_one()` accounts every failed attempt, including the terminal one.
- `RetryContextDto` carries `remaining_millis`, never `Instant`, across
  durable/protocol boundaries. `from_dto` never restores more than carried.
- `UnifiedRetryDisposition::from_provider` maps the provider taxonomy:
  `Transient` → `Transient`, `Permanent` → `Never`,
  `Conditional` → `AuthRefreshable`. `is_retryable()` is true for
  `Transient`, `RateLimited`, and `AuthRefreshable` — **never** for
  `UncertainSideEffect`.
- `UncertainSideEffect.detail` is truncated to 500 chars and must already
  be secret-safe; diagnostics carry chain/domain/operation/key only, never
  arguments, commands, URLs, or credentials.

## Provider (`src/agent/provider_turn.rs`)

- `receive_with_retry_context(loop_, request, Option<RetryContext>)`
  binds to the caller chain; `receive` preserves legacy single-chain
  behavior via `for_provider_turn()` (it is `#[allow(dead_code)]` — the
  chain-bearing entry point is the live one).
- Effective attempts = `min(3, chain.remaining)`; each failure consumes one;
  deadline expiry stops with the last typed error. The provider's own
  ceiling is `MAX_ATTEMPTS = 3` with a 120s setup budget and a 90s
  stream-idle timeout.
- M001 visible-output gate unchanged: replay after visible output is still
  supersession, not retry. Chain ID appears in tracing alongside attempt ID.
- `AgentLoop` (`loop.rs`, `follow_up.rs`) creates one `for_operation()`
  chain per turn and passes it to the provider; tool execution derives
  narrower children from the same chain.

## Tools (`src/tool/retry.rs`, `broker.rs`, `contract.rs`)

`src/tool/retry.rs` owns the decision matrix and returns a
`ToolRetryDecision`:

```rust
pub enum ToolRetryDecision {
    Retry { attempts_left: u8 },
    DoNotRetry { reason: &'static str },
    Uncertain(UncertainSideEffect),
}
```

- `ToolTerminalStatus::UncertainSideEffect` + `ToolValue::uncertain()` is the
  typed ambiguous-commit result. `ProgrammaticOutcome::UncertainSideEffect`
  keeps it out of `CompletedCall`; only `Success` completes.
- `decide_tool_retry(contract, ack, key, error, chain)`:
  - Denied / disabled / backend-unavailable / not-found / format →
    `DoNotRetry`, no budget burn as transient.
  - Non-retryable execution → `DoNotRetry`.
  - Expired/exhausted chain → `DoNotRetry`.
  - Uncertain + `NonIdempotent`/`ProcessExec` → `Uncertain` even when the
    contract disables retry (uncertainty is surfacing, not replay). This
    check runs **before** the `max_retries` gate, so a legacy
    `max_retries=0` contract still yields a typed uncertain result instead
    of a misleading timeout.
  - Uncertain + `IdempotentMutating` without a stable key → `Uncertain`.
  - `ReadOnly`/`ReadValidate`/`SafeRepeat` transient → `Retry`.
  - `IdempotentMutating` transient with a stable key → `Retry` (same key);
    without one → `DoNotRetry` (or `Uncertain` when the ack was lost).
  - `NonIdempotent`/`ProcessExec` before dispatch → bounded `Retry`;
    after dispatch → `Uncertain`.
- `decide_job_retry(idempotency, ack, chain, tool_name)` is the
  **durable-job** counterpart, keyed on `codegg_core::jobs::IdempotencyClass`
  rather than `ToolEffectClass`: only `ReadOnly`/`SafeRepeat` auto-retry;
  every other class retries only when the job was never dispatched
  (`NotDispatched` / `FailedBeforeCommit`) and otherwise becomes
  `Uncertain` asking the caller to reconcile by submission key.
- `ack_for_dispatch_phase(dispatched, error)`: pre-invocation →
  `NotDispatched`; `Timeout`/`Network`/`Io` after dispatch → `Uncertain`;
  explicit rejections → `Acknowledged`.
- `is_effect_retry_eligible(effect)` mirrors
  `ToolEffectClass::is_retry_eligible` at the call site.
- `uncertain_for_tool(chain, tool_name, operation, invocation_key, reason)`
  builds the secret-safe `UncertainSideEffect` (domain `"tool"`, operation
  `"<tool>:<effect-or-label>"`).
- `ToolBroker::execute` preserves legacy single-attempt semantics (it
  delegates to `execute_with_retry` with `None`).
  `execute_with_retry(registry, tool, input, ctx, Option<RetryContext>)`
  validates authority once, reuses the same context (no new permissions,
  no sandbox/model/provider change), keeps a stable invocation key
  (`ctx.submission_key`), backoffs from the contract policy
  (`base_delay_ms` doubled per attempt, capped at `max_delay_ms`) with
  cancellation-aware sleep, and emits chain/disposition tracing without
  payloads. With no chain it runs exactly one attempt; with a chain the
  attempt budget is `min(1 + contract.retry_policy.max_retries,
  chain.attempts_remaining())`.
- `validate_retry_contract` rejects blind retry on
  `NonIdempotent`/`ProcessExec` and keyless `IdempotentMutating` at
  registration time. Legacy `ToolContract::legacy()` stays
  `NonIdempotent` / `max_retries=0` (non-retryable).

## Reconciliation (narrow, per-domain)

- Scheduler: `JobSubmissionService::reconcile_by_key` checks the in-memory
  fast path then the durable store scan (up to 256 records for the
  workspace); fingerprint mismatch is `SubmissionKeyConflict`, never a
  duplicate. `submit` already converges; `reconcile_by_key` is the explicit
  lost-ack path. Durable jobs retain canonical attempt semantics.
- Git: `reconcile_git_operation` returns `NotApplicable` for reads (a
  fixed read-only subcommand allowlist: `status`, `diff`, `log`, `show`,
  `blame`, `branch`, `tag`, `remote`, `worktree`, `stash`, `rev-parse`,
  `for-each-ref`, `fetch`) and pre-dispatch failures;
  `Unreconciled(git:push…)` for uncertain mutations.
  The Git tool must inspect the exact remote/ref via existing read paths
  before any retry.
- External APIs: `reconcile_idempotent_api` returns `Reconciled` only with
  both a stable key and proven backend dedup/status support; with neither it
  returns `NotApplicable` (this domain has no reconciler); with exactly one
  it returns `Unreconciled` (never invents idempotency).

## Observability

Tracing carries `chain_id`, `attempt_index`, `error_class`,
`attempts_left/consumed`, and `uncertain.summary()` (domain/operation/chain/
key). No arguments, commands, or secrets. Bus events unchanged (M001
attempt lifecycle); retry-chain diagnostics are tracing-local to avoid
breaking protocol compat.

## Qualification (M008)

Integrated fault-injection qualification lives in
`tests/agent_loop/reliability_qualification_m008.inc`: transient/permanent/conditional
taxonomy, bounded `Retry-After`, nested-budget exhaustion, cancellation
during backoff, idempotent recovery vs exactly-once uncertain surfacing,
and secret-safe diagnostics. No production change; the suite proves the
M001+M002 contract holds under injected faults.

A second, narrower suite —
`tests/agent_loop/retry_budget_reconciliation.inc` — covers the broker and
scheduler legs directly: idempotent write retried with the same key,
non-idempotent dispatched timeout surfacing as uncertain, a raw-shell
ambiguous call not being replayed, legacy single-attempt behavior without a
chain, permission denial never retrying, chain-budget exhaustion,
cancellation during the chain, and durable submission reconciling after a
lost acknowledgement (both in-memory and after a restart that reconciles
from the durable store).

## Testing

```bash
cargo test --test agent_loop -- reliability_qualification_m008
cargo test --test agent_loop -- retry_budget_reconciliation
cargo test -p codegg-providers --lib retry
cargo test --lib tool::retry
```

## Related Docs

- [resilience.md](resilience.md) — provider circuit-breaker / backoff resilience
- [provider.md](provider.md) — provider wire, catalog, and discovery
- [scheduler.md](scheduler.md) — durable job admission and submission
- [tool_broker.md](tool_broker.md) — the broker pipeline retry runs inside
- [error.md](error.md) — the typed error taxonomy the disposition maps from

## What is out of scope

No semantic rollback of arbitrary shell, no distributed transactions, no
invented API idempotency, no scheduler-attempt ownership change, no
approval/sandbox or provider-failover change.

## Source verification

Verified 2026-10-23 against `crates/codegg-providers/src/retry.rs`,
`src/tool/{retry,broker,contract}.rs`, `src/scheduler/submission.rs`,
`src/agent/{provider_turn,loop,follow_up}.rs`, and
`tests/agent_loop/{reliability_qualification_m008,retry_budget_reconciliation}.inc`.

Corrections made in this pass:
- **Corrected the chain-contract listing.** `RetryContext` has **five**
  fields — the listing omitted the internal `created_at`.
  `ReconciliationOutcome::Reconciled` carries a `detail: String` payload,
  and `UnifiedRetryDisposition::RateLimited` carries
  `retry_after: Option<Duration>`; both were shown as bare unit variants.
- **Corrected `single_attempt()`'s numbers**: it is 1 attempt / 60s, not
  an unstated default.
- **Corrected the tool decision matrix.** The doc listed
  "Denied/disabled/not-found/format" as the permanent set; the source also
  treats `ToolError::BackendUnavailable` as a permanent denial. Added the
  `max_retries` gate ordering (the uncertainty check runs *before* it).
- **Added the missing durable-job leg.** `decide_job_retry` (keyed on
  `codegg_core::jobs::IdempotencyClass`) was undocumented, as were
  `is_effect_retry_eligible` and `uncertain_for_tool`.
- **Added the `ToolRetryDecision` type**, which the tool section described
  only by its three arms.
- **Corrected the Git reconciler's read-only list** (it is a fixed
  allowlist of 13 subcommands) and the **external-API reconciler's third
  arm**: with neither a stable key nor backend dedup support it returns
  `NotApplicable`, not `Unreconciled`.
- **Corrected the broker's attempt-budget formula** to
  `min(1 + max_retries, chain.attempts_remaining())` and named the backoff
  policy (doubling from `base_delay_ms`, capped at `max_delay_ms`).
- **Corrected `ProviderTurnAdapter::receive`**: it is
  `#[allow(dead_code)]` — the chain-bearing entry point is the live one.
- **Added the provider's own ceilings** (`MAX_ATTEMPTS = 3`, 120s setup
  budget, 90s stream-idle timeout).
- **Added the second qualification suite** and a `## Testing` section with
  the real commands.
- **Added `## Purpose` / `## Where It Lives` / `## Related Docs`** to match
  the shape of the other architecture docs.

Confirmed explicitly: `MAX_CHAIN_ATTEMPTS = 8` (`retry.rs:17`),
`MAX_CHAIN_DURATION = 300s` (`:20`), `MAX_UNCERTAIN_DETAIL_CHARS = 500`
(`:24`), `RetryContext`'s 5 fields (`:55`),
`single_attempt()`/`for_provider_turn()`/`for_operation()` = 1/60s, 3/120s,
6/180s (`:81,:87,:93`), the exact `AckState` (4),
`UnifiedRetryDisposition` (5), `UncertainSideEffect` (5 fields), and
`ReconciliationOutcome` (3) variant listings, `derive_child` (`:143`),
`consume_one` (`:132`), `RetryContextDto` (`:190`);
`decide_tool_retry` (`src/tool/retry.rs:61`),
`ack_for_dispatch_phase` (`:38`),
`reconcile_git_operation` (`:262`),
`reconcile_idempotent_api` (`:306`),
`validate_retry_contract` (`:345`);
`ToolBroker::execute` (`broker.rs:752`) and `execute_with_retry` (`:774`);
`ProgrammaticOutcome::UncertainSideEffect` (`broker.rs:494`, mapped from
`ToolTerminalStatus::UncertainSideEffect`);
`receive`/`receive_with_retry_context` (`provider_turn.rs:49,56`);
`JobSubmissionService::submit` (`submission.rs:250`) and
`reconcile_by_key` (`:494`); the one `for_operation()` chain per turn
(`loop.rs:1467`, `follow_up.rs:499`); and both qualification suites.
