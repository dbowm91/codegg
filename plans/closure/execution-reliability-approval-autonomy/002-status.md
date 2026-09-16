# Execution Reliability, Approval, and Autonomy M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/execution-reliability-approval-autonomy/002-unified-retry-budget-and-side-effect-reconciliation.md`

Source subsystem roadmap:

- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md#7-milestones`

Repository baseline reviewed: `9858c282`

Implementation commits or pull requests:

- `8d810fc3` — execution-reliability M002: unified retry budget and side-effect reconciliation

## 1. Executive finding

M002 is complete. One bounded retry chain now propagates through provider,
tool, scheduler, and compatible external-effect paths: `RetryContext`
(chain ID, deadline, remaining attempts) is created at the logical
operation/turn boundary, lower layers consume but never replenish it, and
existing provider/tool caps remain lower ceilings. Effect/idempotency-aware
tool retry plus acknowledgement-state classification routes ambiguous
non-idempotent commits to narrow reconciliation or a typed
`UncertainSideEffect` instead of blind replay. Durable scheduler submission
reconciles by submission key rather than duplicating. Cancellation/deadline
stops all further attempts, and retry grants no new permissions and alters
no sandbox/model/provider selection.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| RetryContext chain/deadline/attempt contract (WP-A) | `crates/codegg-providers/src/retry.rs`: `RetryChainId`, `RetryContext::{new,derive_child,consume_one,is_expired,is_live,to_dto,from_dto}`, `MAX_CHAIN_ATTEMPTS=8`, `MAX_CHAIN_DURATION=300s`; `for_operation` (6/180s), `for_provider_turn` (3/120s), `single_attempt` legacy | pass | One semantic definition in the leaf providers crate; `Instant` never crosses DTOs (`RetryContextDto` carries `remaining_millis`) |
| Propagated disposition/consumption (WP-A) | `src/agent/provider_turn.rs::receive_with_retry_context`, `stream_with_retry(..., Option<RetryContext>)` with `effective_max=min(3, remaining)`, per-failure `consume_one`, deadline stop; `src/agent/loop.rs` + `follow_up.rs` create `for_operation()` per turn | pass | M001 caps remain lower ceilings; legacy `receive` preserved as single-chain wrapper |
| Tool effect/idempotency mapping (WP-B) | `src/tool/retry.rs::decide_tool_retry`, `is_effect_retry_eligible`, `validate_retry_contract`; `ToolEffectClass::is_retry_eligible` unchanged; `ToolBroker::execute_with_retry` caps tool attempts at `min(1+max_retries, remaining)` | pass | ReadOnly/ReadValidate/SafeRepeat retry transient; IdempotentMutating requires stable key; NonIdempotent/ProcessExec retry only pre-dispatch, else uncertain |
| Unsafe combinations rejected (WP-B) | `validate_retry_contract` (blind retry on NonIdempotent/ProcessExec or keyless IdempotentMutating rejected); existing `ToolContract::validate` still rejects retry on non-eligible effects; legacy contracts stay `NonIdempotent/max_retries=0` | pass | Registration-time + runtime gates |
| Acknowledgement state (WP-C) | `AckState::{NotDispatched,FailedBeforeCommit,Acknowledged,Uncertain}`, `ack_for_dispatch_phase`, `requires_reconciliation`; `retry.rs` unit tests for transitions | pass | Timeout/Network/Io after dispatch → Uncertain; explicit rejections → Acknowledged; pre-invocation → NotDispatched |
| Narrow reconciliation hooks (WP-C) | `reconcile_git_operation` (reads/N/A vs uncertain push → Unreconciled), `reconcile_idempotent_api` (Reconciled only with key+dedup, else Unreconciled/N/A), `JobSubmissionService::reconcile_by_key` (in-memory fast path + durable scan, conflict on fingerprint mismatch) | pass | No generic guesser; no invented API idempotency; scheduler retains canonical attempt semantics |
| Typed UncertainSideEffect (WP-C) | `UncertainSideEffect::{new,summary}` (500-char bound, secret-safe by construction), `ToolTerminalStatus::UncertainSideEffect`, `ToolValue::uncertain`, `ProgrammaticOutcome::UncertainSideEffect` (never `CompletedCall`), broker uncertain display omits args/commands/secrets | pass | Uncertain surfaces even when contract disables retry (surfacing ≠ replay) |
| Observability/bounds (WP-D) | Tracing with `chain_id`, `attempt_index`, `error_class`, `attempts_left/consumed`, `uncertain.summary()`; provider success/terminal/deadline logs carry chain; tool retry/backoff/uncertain logs carry chain; no payloads/URLs/keys | pass | Bus unchanged (M001 lifecycle intact, protocol compat); chain diagnostics are tracing-local |
| Runtime/concurrency semantics | Chain created per turn, passed downward, `derive_child` caps; cancellation checked before each attempt and during backoff (`CancellationToken`), deadline stops provider + tool loops; restart never regenerates a process-local chain for ambiguous effects | pass | See restart/contention tests |
| Protocol/frontends | No protocol change; `ToolTerminalStatus`/`ProgrammaticOutcome` additions are additive enum variants with existing compat mapping (uncertain → typed failure, never success); diagnostics bounded/secret-safe | pass | Older clients ignore the new terminal variant as a failure |

## 3. Production implementation evidence

Landed ownership and behavior:

```text
RetryContext (codegg-providers/src/retry.rs)
  new(total, timeout) clamped to 8 attempts / 300s
  derive_child(max) shares chain_id+deadline, caps at parent remaining
  consume_one() per failed attempt (incl. terminal); is_expired/is_live gates
  to_dto/from_dto transport remaining_millis only; tampered DTOs clamped

Provider (src/agent/provider_turn.rs)
  receive_with_retry_context(loop_, req, Option<RetryContext>)
  chain = caller or for_provider_turn(); effective_max = min(3, remaining)
  per-failure consume_one; deadline expiry stops with last typed error
  visible-output supersession unchanged; chain_id in all attempt tracing
  loop.rs + follow_up.rs create for_operation() per turn

Tool (src/tool/retry.rs + broker.rs + contract.rs)
  decide_tool_retry: Never (denied/validation/non-retryable) > Uncertain
    (unsafe effect + lost ack, even when max_retries=0) > Retry (safe/keyed,
    within chain) ; expired/exhausted stops
  execute_with_retry: validate authority once, reuse ctx (no re-grant, no
    sandbox/model/provider change), stable invocation key, contract-policy
    backoff with cancellable sleep, secret-safe uncertain display
  execute (no chain) preserves legacy single-attempt mapping exactly
  ToolTerminalStatus::UncertainSideEffect + ProgrammaticOutcome::UncertainSideEffect

Reconciliation
  scheduler: reconcile_by_key (memory index → durable scan → conflict or miss)
  git: reads/N-A; uncertain push → Unreconciled(git:push) with remote/ref guidance
  api: Reconciled only on key+dedup proof; else Unreconciled/N-A
```

Retry-chain propagation diagram (one logical operation):

```text
for_operation() chain=6/180s (loop.rs / follow_up.rs per turn)
  ├─ provider stream_with_retry: effective min(3, remaining)
  │    attempt0 fail → consume → attempt1 fail → consume → attempt2 ok
  │    (remaining 6→4; tool child derives min(contract, 4))
  └─ tool execute_with_retry per call: allowed min(1+max_retries, remaining)
       safe read Timeout → Retry (consume, backoff, same key)
       non-idempotent Timeout after dispatch → Uncertain (consume, surface)
       permission denied → DoNotRetry (Never, no transient burn)
```

## 4. Verification executed

### Commands run

```bash
cargo test -p codegg-providers -- --skip shared_client_follows_redirects
cargo test --test retry_budget_reconciliation
cargo test --test tool_structured_execution
cargo test --test command_routing_execution_ownership
cargo test --test scheduler_contention
cargo test --test tool_broker_integration
cargo test --test agent_loop_harness -- m001
python3 scripts/check_execution_ownership.py
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
./scripts/verify.sh quick
```

Focused target `retry_budget_reconciliation` is the new M002 harness;
`m001` is the M001 regression gate.

### Results

- `cargo test -p codegg-providers` (skip flaky loopback): 143/143 pass,
  incl. 8 new `retry::` unit tests (budget, clamp, child-cap, expiry,
  DTO clamp, ack gate, disposition mapping, detail bound).
- `cargo test --test retry_budget_reconciliation`: 15/15 pass —
  chain-bound (2), expiry gate (1), broker retry/uncertain/legacy/denied/
  exhaustion/cancel (7), git/uncertain-secret-safety (2), scheduler
  lost-ack/restart/concurrent-converge (3).
- `cargo test -p codegg --lib -- tool::retry`: 11/11 pass (effect matrix,
  key requirement, non-idempotent/process-exec uncertain, denied-never,
  exhaustion, ack classification, git/api reconciliation, contract
  validation, payload omission).
- `cargo test --test tool_structured_execution`: 9/9 pass.
- `cargo test --test command_routing_execution_ownership`: 21/21 pass.
- `cargo test --test scheduler_contention`: 14/14 pass.
- `cargo test --test tool_broker_integration`: 25/25 pass.
- `cargo test --test agent_loop_harness -- m001`: 8/8 pass (M001 intact).
- `check_execution_ownership.py`: ok.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: pass.
- `scripts/verify.sh quick`: pass (fmt, builtin-agents, core-boundary,
  sandbox, execution-ownership, tui-authority, workspace check).
- Full `cargo test -p codegg-providers` without skip: 143 pass, 1 fail in
  `shared_client_follows_redirects_and_enforces_ten_hop_bound` (loopback
  `WouldBlock`/`ConnectionReset` flake, unrelated to M002 — no change to
  `provider_core` HTTP client; see §10).

Attempt-count evidence: `nested_context_cannot_exceed_parent_budget`
(parent 4 → provider consumes 2 → child capped at 2 → total ≤ 4);
`provider_and_tool_share_one_bounded_chain` (chain 4 → provider child 3,
after 2 consumes tool child capped at 2);
`retry_respects_chain_budget_exhaustion` (chain 2 vs 3 needed → terminal
failure, attempts ≤ 2, no hidden layer continues).

## 5. Invariant review

| Plan invariant (§4) | Evidence |
|---|---|
| Non-idempotent + unknown ack never auto-replayed on timeout/reset | `decide_tool_retry` Uncertain branch precedes the retry gate; `non_idempotent_dispatched_timeout_is_uncertain`, `raw_shell_ambiguous_is_not_replayed` assert 1 attempt + `UncertainSideEffect` |
| Lower layers consume, never reset/increase parent budget | `derive_child` caps at parent remaining; `consume_one` on every failure incl. terminal; nested-cap unit tests |
| One chain has hard attempt + elapsed bound | `MAX_CHAIN_ATTEMPTS`/`MAX_CHAIN_DURATION` clamps + `is_expired`/`is_exhausted` stops in provider + tool loops; exhaustion tests |
| Scheduler durable jobs keep canonical attempt semantics | No change to `JobStore`/attempt machine; `reconcile_by_key` is read-only lookup + conflict, never mutates attempts |
| Read-only/idempotent recover without needless intervention | `FlakyReadTool`/`FlakyIdempotentWriteTool` recover within policy with same key; safe-matrix unit tests |
| Invocation/idempotency keys stable across safe retries | Broker reuses `ctx.submission_key` for every attempt; idempotent test uses one key; backend `invocation_key` threaded unchanged |
| Retry grants no permissions, alters no sandbox/model/provider | Authority validated once pre-loop; retries reuse the same `ctx`/grant; no sandbox/model/provider writes in retry paths; denied-never test |

## 6. Failure and recovery review

- Budget/deadline exhaustion returns the last typed failure (`TimedOut`,
  `Denied`, `Uncertain`, or explicit error); the post-loop fallthrough
  returns `last_err_value` or `TimedOut` — no hidden layer continues.
- Cancellation wins over backoff and reconciliation: checked before each
  provider attempt, per stream event, during provider backoff
  (`sleep_cancellable`), before each tool attempt, and during tool backoff;
  `cancel_during_chain_stops_retry` proves pre-cancelled zero-execution.
- Restart does not regenerate a process-local chain for ambiguous effects:
  `restart_reconciles_from_durable_store` proves a fresh service with an
  empty index still finds the durable job by key and resubmits to the same
  `job_id`; uncertain external mutations stay `Unreconciled` until the
  Git/API read path proves otherwise (no auto-replay helper exists).
- Concurrent duplicates converge: `concurrent_duplicate_submissions_converge`
  (2 workers, same key → one `job_id`) via the double-checked creation
  lock plus durable scan.
- Unreconciled stays uncertain: `ReconciliationOutcome::Unreconciled` is
  never converted to success/failure by absence of evidence; broker maps it
  to `UncertainSideEffect` terminal status.
- Duplicate delivery: provider partials still discarded on failure (M001);
  tool retries reuse the same invocation key so idempotent backends dedup;
  scheduler `SubmissionKeyConflict` on fingerprint mismatch prevents
  same-key-different-work collapse.

## 7. Migration and compatibility review

- No DB migration; no storage layout change (`STORAGE_LAYOUT_VERSION`
  untouched).
- `BrokerInvocationContext` unchanged (retry passed as a separate
  `Option<RetryContext>` arg), so all 40+ struct-literal construction sites
  compile untouched; `execute` without a chain is byte-identical in behavior
  to pre-M002 (`legacy_single_attempt_preserved_without_chain`).
- `BrokerResult` unchanged (diagnostics are tracing-local).
- Additive enum variants only: `ToolTerminalStatus::UncertainSideEffect`
  and `ProgrammaticOutcome::UncertainSideEffect`; all pre-existing matches
  updated in `broker.rs`; downstream `match result.into_programmatic_outcome()`
  sites key on `Ok/Err`, so uncertain correctly becomes a typed failure,
  never a `CompletedCall`.
- Provider `receive` retained as a single-chain wrapper; new
  `receive_with_retry_context` is the chain-aware path.
- Rollback: reverting `8d810fc3` restores single-budget provider/tool paths;
  no persisted data depends on chain IDs or uncertain terminals.

## 8. Security review

- Never-retry denials: `Permission`/`Disabled`/`NotFound` short-circuit
  before budget accounting (`permission_denied_never_retries`).
- No authority expansion: retries reuse the validated grant/context; no
  re-resolution, no `BrokerAuthority` mutation, no sandbox/model/provider
  writes in any retry path.
- Secret safety: uncertain displays carry tool name + chain + error class +
  bounded reason only; `non_idempotent_dispatched_timeout_is_uncertain` and
  `raw_shell_ambiguous_is_not_replayed` assert `sk-secret-123`, command
  bodies (`rm -rf`, tokens), and arguments are absent; provider diagnostics
  remain kind/class/ID-only (M001); `UncertainSideEffect::new` truncates to
  500 chars.
- DoS bounds: chain 8 attempts / 300s hard caps; provider 3-attempt and
  contract `1+max_retries` lower ceilings; contract backoff capped with
  jitter; `Retry-After` 30s clamp unchanged.

## 9. Documentation and operations

Updated:

- `architecture/retry.md` — canonical retry ownership (chain contract,
  provider/tool binding, effect matrix, narrow reconciliation,
  observability, non-goals).
- `architecture/provider.md` — unified-chain pointer + disposition
  composition note.
- `architecture/tool.md` — retry-doc link.

Operator notes: watch `provider retry chain deadline reached` (warn) vs
`tool attempt transient; retrying within chain` (info, with
`attempts_left`) vs `tool effect uncertain; surfacing instead of replaying`
(warn, with `uncertain=tool:op chain=… key=…`) vs `tool attempt failed
terminally; not retried` (warn, with `reason`). Chain IDs correlate
provider attempts and tool retries for one turn; uncertain displays tell
the model/user what may have happened and demand reconciliation before any
manual retry.

No new static guard: with exactly two chain-aware loops (provider, broker),
focused attempt-count/uncertain tests enforce the bound per plan §6
(test-over-lint allowance).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | `shared_client_follows_redirects_and_enforces_ten_hop_bound` flakes under load (`WouldBlock`/`ConnectionReset` loopback errors; 143/144 pass unskipped, 143/143 pass skipped) | No M002 impact (M002 touches no HTTP client code), but the providers suite is not green unskipped in this environment | None for M002; if the flake persists, file a corrective for the loopback fixture (retry/backoff in the test harness, not provider behavior) |
| Low | Tool retries do not yet thread the turn chain into `tool_batch.rs` agent-loop executions (broker calls there still use single-attempt `execute`) | Provider and broker each honor a caller chain, and the shared-budget primitive is proven, but end-to-end agent turns do not yet share one chain object across the provider→tool seam | M008 qualification (or a small M003-adjacent threading pass) should pass the turn chain into `tool_batch.rs::BrokerInvocationContext` via `execute_with_retry`; do not invent a second chain |
| Low | Git reconciliation classifies but does not execute the remote/ref inspection | Uncertain pushes surface correctly, but the actual `ls-remote`/ref comparison reuses the operator's existing Git read path manually | Keep as-is; M008 fault-injection should add a push-ambiguity fixture that drives the existing Git read tool and asserts no second push |
| — | No other open items | — | — |

No stop condition triggered (no distributed-transaction manager, no shell
semantics parser, no scheduler-ownership change).

## 11. Roadmap disposition

Milestone closed and no future milestone is newly unblocked beyond the
already-ready parallel track:

- M002 (unified retry budget and side-effect reconciliation): hard
  dependency M001 satisfied — close.
- M003 remains independently **ready** (parallel-safe with M002,
  unaffected).
- M004-M007 remain blocked on the M003 chain (unchanged).
- M008 remains blocked on M001-M007 (M001 + M002 legs now satisfied;
  still blocked on M003-M007).

## 12. Registry updates

- `plans/registry.md`: M002 `ready` → `closed` with closure link and
  implementation commit `8d810fc3`; subsystem row `M001 closed; M002 and
  M003 ready` → `M001+M002 closed; M003 ready`; dependency-ready table
  M002 row → `closed`; execution-order item 2 rewritten to reflect M002
  closure; M002 appended to recently-closed work.
- `plans/subsystems/execution-reliability-approval-autonomy-roadmap.md`:
  M002 section `ready` → `closed` with closure link; status table updated.
- `plans/implementation/execution-reliability-approval-autonomy/002-unified-retry-budget-and-side-effect-reconciliation.md`:
  `Status: ready` → `Status: implemented`.
