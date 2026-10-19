# M005 Writer-Containment Corrective C002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md` (C002)
Repository baseline reviewed: `aa65cf7c` (post-C001, post-ADR-0042 acceptance `37a82843`)
Implementation commits: see §3
Decision: `plans/adrs/ADR-0042-restricted-agent-shared-checkout-writer-contract.md` (**accepted 2026-10-19, option B**)

## 1. Executive finding

C002 shipped the durable, generation-fenced checkout writer permit and its
lifecycle service, and nothing else.

The permit is a real, working authority: one durable row per managed checkout,
a four-state machine (`Available` / `Held` / `Releasing` / `Quarantined`), a
single-owner transactional compare-and-swap under a monotonic revision, a
non-duplicable identity-bound owner token, a lease-generation fence, idempotent
acquisition, evidence-gated release, quarantine with operator-only recovery,
restart reconciliation, a bounded per-checkout waiter, and a redacted operator
view. 21 store tests and 12 adapter tests exercise it against a real SQLite
database, including N=2 and N=16 contention, ABA, stale revision/token/
generation, altered-payload retry, cancellation and timeout while waiting,
transaction rollback, a panicking holder, expiry, restart, and close/reopen.

**C002 is not shared-write readiness, and it did not become one.** No writer is
routed through the permit. No shared-write mode, no user-visible
shared-mutation selector, and no user-facing enablement exists. The isolated
worktree remains the default placement for mutating agents, and accepted
ADR-0002 and ADR-0032 are unmodified.

## 2. Requirement-to-evidence matrix

| Plan requirement | Where satisfied | Evidence |
|---|---|---|
| A — explicit states `Available` / `Held(owner, generation, attempt, operation)` / `Releasing` / `Quarantined(reason)` | `crates/codegg-core/src/writer_permit.rs` (`WriterPermitState`, `WriterPermitRecord`) | `two_acquirers_produce_exactly_one_holder`; `unproven_quiescence_quarantines_instead_of_releasing` |
| A — on acquisition verify active group reservation, current lease generation, project/principal/run auth | `src/core/writer_permit.rs` (`WorktreePermitAuthority::resolve_scope`) | `the_lease_owner_may_acquire`; `a_run_without_the_lease_is_refused` |
| A — reject unknown/stale IDs | `WriterPermitError::{NotFound, StaleRevision, StaleLeaseGeneration}` | `a_stale_revision_cannot_release`; `a_stale_lease_generation_cannot_release`; `a_stale_owner_token_cannot_release` |
| A — single-owner transactional CAS with monotonic revision | `WriterPermitStore::acquire` under `ImmediateWrite` (`BEGIN IMMEDIATE`); the acquire's `UPDATE` carries `state = 'available' AND revision = ?` in its `WHERE` clause, and the release/quarantine updates carry `revision` **and** `lease_generation` | `sixteen_concurrent_acquirers_produce_exactly_one_holder`; `the_store_is_usable_from_many_connections` |
| A — idempotent invocation handling | `AcquireWriterPermit::idempotency_key` + scope fingerprint | `an_idempotent_retry_returns_the_same_permit`; `an_idempotent_retry_with_an_altered_payload_is_refused` |
| A — versioned schema/migration | `migrate_v84` (`writer_permit` table, additive, with SQL `CHECK` bounds on every persisted string and on the fence columns); `STORAGE_LAYOUT_VERSION` 83 → 84 | `scripts/check_project_catalog_invariants.py` (7/7) |
| B — bounded async waiter, integrated with cancellation, no second admission queue | `WriterPermitService::wait_for_permit`; per-checkout FIFO, depth-capped, timeout + `CancellationToken` | `a_waiter_takes_the_permit_when_it_is_released`; `a_cancelled_wait_gives_up_without_taking_the_permit`; `a_timed_out_wait_gives_up_without_taking_the_permit` |
| B — distinct worktrees proceed independently | Per-checkout queues; the permit is keyed by checkout | `distinct_worktrees_acquire_in_parallel`; `distinct_checkouts_do_not_wait_on_each_other` |
| B — no global worktree-mutation key as the safety gate | The permit has its own durable store; the scheduler's exclusivity keys are **not** used (they are empty for `Shell`/`ManagedProcess`/`Test`/`Lint`/`Python`/`ToolProgram` and only `exclusive:`-prefixed keys conflict) | `scripts/check_scheduler_bypass.py` (ok); the boundary is documented in `.opencode/skills/scheduler/SKILL.md` and `architecture/writer_permit.md` |
| B — waiters cannot starve other groups indefinitely | FIFO per checkout; only the queue head may attempt the CAS | `a_waiter_takes_the_permit_when_it_is_released` |
| B — cancellation/permission wait must not leak a writer token | A waiter holds nothing while it waits; a permit taken in the same instant the wait is cancelled is released with `NoEffect` evidence | `a_cancelled_wait_gives_up_without_taking_the_permit`; `a_timed_out_wait_gives_up_without_taking_the_permit` |
| B — no permit held during inference, approval, or nested submission | Contractual: the guard is scoped to one critical section and is settled explicitly. C002 has **no** production caller, so there is no path that can hold a permit across a phase | `architecture/writer_permit.md` ("The holder contract") |
| B — nested reuse only via identity-bound non-duplicable transfer | `WriterPermitTransfer`; the successor gets a new token in the same statement that invalidates the old one | `a_transfer_invalidates_the_previous_holder` |
| C — normal release only after the checked transaction resolved | Two-phase release: `begin_release` → `complete_release` | `restart_reconciliation_quarantines_an_interrupted_release` |
| C — positive quiescence evidence before release | `QuiescenceEvidence::{Settled, NoEffect, Unproven}` | `unproven_quiescence_quarantines_instead_of_releasing`; `unproven_quiescence_parks_the_permit` |
| C — aborted / incomplete / process-tree uncertain / crash / expired lease → `Quarantined` | `WriterPermitQuarantineReason` (8 variants) | `unproven_quiescence_quarantines_instead_of_releasing`; `restart_reconciliation_quarantines_a_live_permit`; `an_expired_lease_does_not_free_the_permit` |
| C — lease deadline alone is NEVER enough to free a live writer | Expiry is a stored hint only; the store has no expiry-driven release path at all | `an_expired_lease_does_not_free_the_permit` |
| D — restart reconciliation; do not replay a non-idempotent mutation | `WriterPermitStore::reconcile_after_restart`; the daemon runs it at startup | `restart_reconciliation_quarantines_a_live_permit`; `startup_reconciliation_parks_a_permit_left_live` |
| D — typed operator inspect/release only after verified quiescence | `WriterPermitService::{inspect, list, operator_release, operator_quarantine}` | `the_operator_can_inspect_and_release_a_parked_permit`; `the_operator_release_needs_proven_quiescence` |
| D — never silently delete dirty files | The permit never touches the checkout; operator release frees the permit only | `the_operator_can_inspect_and_release_a_parked_permit` |
| D — bounded event/projection data with redaction | `WriterPermitView`; the possession token is never projected and the operation is a closed enum | `the_operator_view_carries_no_free_text` |
| E — codegg-core typed IDs / store / state machine | `crates/codegg-core/src/writer_permit.rs` | `scripts/check-core-boundary.sh` (passed) |
| E — root daemon adapter supplies authorization | `src/core/writer_permit.rs` (`WorktreePermitAuthority`) | `the_lease_owner_may_acquire`; `a_run_without_the_lease_is_refused` |
| E — WorktreeService coordination | The authority reads the worktree's current lease generation and owner run from `WorktreeService` | `the_lease_owner_may_acquire` |
| E — separate permit from scheduling capacity and native tool authorization | The permit is a separate store with its own CAS; it is not a tool, not a tool capability, and not a scheduler resource | `scripts/check_scheduler_bypass.py` (ok); `scripts/check_authorization_matrix.py` (all invariants verified) |

### Required qualification

| Required | Result |
|---|---|
| Two-run N=2 CAS contention (one holder) | `two_acquirers_produce_exactly_one_holder` |
| N=16 CAS contention (one holder) | `sixteen_concurrent_acquirers_produce_exactly_one_holder` |
| Different-worktree parallelism | `distinct_worktrees_acquire_in_parallel`; `distinct_checkouts_do_not_wait_on_each_other` |
| Stale owner / generation / ABA | `a_stale_owner_token_cannot_release`; `a_stale_lease_generation_cannot_release`; `a_stale_revision_cannot_release`; `a_reacquired_permit_invalidates_the_previous_holder`; `a_permit_held_under_an_old_generation_blocks_a_new_generation` |
| Duplicate retry with altered payload | `an_idempotent_retry_with_an_altered_payload_is_refused` |
| Cancellation while waiting | `a_cancelled_wait_gives_up_without_taking_the_permit` |
| Panic / transaction rollback | `a_panicking_holder_leaves_the_permit_held`; `an_uncommitted_transaction_rolls_back` |
| Expiry and restart | `an_expired_lease_does_not_free_the_permit`; `restart_reconciliation_quarantines_a_live_permit`; `restart_reconciliation_quarantines_an_interrupted_release` |
| Release after worktree ownership change | `a_permit_held_under_an_old_generation_blocks_a_new_generation`; `a_reacquired_permit_invalidates_the_previous_holder` |
| Cross-principal / project denial | `a_run_without_the_lease_is_refused` (a run that does not hold the lease cannot write; a run in another project cannot hold this checkout's lease) and `a_principal_outside_the_project_is_refused` (a viewer-role principal and a principal with no membership are both refused, through the `file.modify` capability check) |
| Hard stop / quarantine when process completion is unproven | `unproven_quiescence_quarantines_instead_of_releasing`; `unproven_quiescence_parks_the_permit` |
| SQLite close/reopen and competing transactions | `a_quarantine_survives_a_close_and_reopen`; `the_store_is_usable_from_many_connections` |
| No held permit while waiting for job admission | The permit's own waiter holds nothing while it waits (`a_waiter_takes_the_permit_when_it_is_released` asserts the holder is unchanged while the waiter is parked) |
| No user-visible shared-mutation selector activated | There is no `CoreRequest` variant, no CLI command, no protocol surface and no config key for shared mutation. The permit is reachable only from the daemon's own service |

## 3. Production implementation evidence

**New — `crates/codegg-core/src/writer_permit.rs`** (the authority)

- `WriterPermitScope` — the typed tuple: project, repository, workspace,
  worktree, lease generation, group, principal, agent run, attempt. Its
  `fingerprint()` is what an idempotency key is bound to.
- `WriterPermitOwner` — principal, run, group, attempt, and the
  non-duplicable possession token.
- `WriterPermitState` — `Available` / `Held` / `Releasing` / `Quarantined`.
- `WriterPermitQuarantineReason` — `DaemonRestart`, `AbortedOperation`,
  `IncompleteEffects`, `ProcessTreeUncertain`, `LeaseExpired`,
  `OwnershipChanged`, `ConflictingEffect`, `OperatorHold`.
- `QuiescenceEvidence` — `Settled` / `NoEffect` / `Unproven(reason)`.
- `WriterPermitStore` — `acquire`, `begin_release`, `complete_release`,
  `quarantine`, `operator_release`, `load`, `list`, `reconcile_after_restart`.
  Every read-then-write path opens its transaction with `BEGIN IMMEDIATE`, and
  every mutating statement carries its compare-and-swap predicate in the
  `WHERE` clause — the expected revision, the expected lease generation, the
  expected owner token, and the expected state — rather than trusting a
  decision taken from an earlier read.
- `WriterPermitView` — the redacted operator projection.

**New — `crates/codegg-core/src/storage/immediate.rs`** (shared primitive)

The `BEGIN IMMEDIATE` write transaction was extracted out of
`crates/codegg-core/src/session/user_prompt_queue.rs` into a shared module, so
the permit store and the queue store use **one** mechanism instead of forking a
subtle one. The queue store keeps a thin `begin_immediate` wrapper, and
`scripts/check_queue_write_transaction_shape.py` was repointed at the
primitive's new home for the rollback contract and the `BEGIN IMMEDIATE`
check. Both of the guard's rules are preserved and its self-test still proves
it rejects a deferred read-then-write path and a closing rollback.

**New — `src/core/writer_permit.rs`** (the daemon adapter)

- `WriterPermitAuthority` (trait) and `WorktreePermitAuthority` (the daemon's
  implementation, over `WorktreeService`, `AgentRunGroupService` and
  `AuthorizationService`).
- `WriterPermitService` — acquisition, the bounded per-checkout waiter, the
  operator surface, and restart reconciliation.
- `WriterPermitGuard` — the holder contract. Release and quarantine are
  explicit; **dropping** an unsettled guard quarantines the permit rather than
  freeing it.

**Changed**

- `crates/codegg-core/src/lib.rs` — `pub mod writer_permit;`.
- `crates/codegg-core/src/session/schema.rs` — `migrate_v84`: the
  `writer_permit` table and its three indexes, with SQL `CHECK` bounds on every
  persisted string (`owner_attempt_id` ≤ 128, `owner_token` ≤ 64,
  `operation` ≤ 64, `idempotency_key` 1–128, `request_fingerprint` ≤ 1024,
  `quarantine_detail` ≤ 512), the state enumeration, the half-written-owner
  check, and `lease_generation >= 1`. The same bounds are enforced in Rust
  before the write.
- `crates/codegg-core/src/storage/mod.rs` — `STORAGE_LAYOUT_VERSION` 83 → 84.
- `crates/codegg-core/src/session/user_prompt_queue.rs` — uses the shared
  `ImmediateWrite`.
- `src/core/mod.rs` — `pub mod writer_permit;`.
- `src/core/runtime_deps.rs` — the daemon owns exactly one permit service; the
  pool-backed constructor installs it; a `with_writer_permit_service` builder
  was added. A pool-less daemon gets **none**, because an in-memory permit
  would be a second, weaker answer to "who may write".
- `src/core/daemon.rs`, `src/core/daemon_construct.rs` — the startup
  reconciliation task.
- `scripts/check_queue_write_transaction_shape.py` — repointed at the shared
  primitive (see above).
- `architecture/writer_permit.md` (new), `architecture/overview.md`,
  `.opencode/skills/scheduler/SKILL.md`.

**No `src/tool/**` file changed, and no writer was routed.** The permit's
consumer is the daemon's own service, which is what "a daemon consumer" in the
plan's closure bar requires; C003 and C004 are what make writers consult it.

## 4. Verification executed

All commands were run in the worktree
`/home/sugarwookie/projects/codegg/.worktrees/m005-writer-audit` at
`aa65cf7c` plus the C002 changes.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo test -p codegg-core --test writer_permit` | **21 passed**, 0 failed |
| `cargo test --test writer_permit_service` | see §4.1 |
| `cargo check -p codegg-core --all-targets` | clean |
| `cargo check --lib` | clean |
| `bash scripts/check-core-boundary.sh` | `codegg-core boundary check passed` |
| `python3 scripts/check_scheduler_bypass.py` | `scheduler-bypass guard ok` |
| `python3 scripts/check_authorization_matrix.py` | `All authorization matrix invariants verified.` |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 checks passed |
| `python3 scripts/check_adr_integrity.py` | `ADR integrity fixtures passed; active ADR IDs are unique and local paths resolve` |
| `python3 scripts/check_queue_write_transaction_shape.py` | all checks pass |
| `python3 scripts/check_queue_write_transaction_shape.py --self-test` | injected deferred read-then-write path rejected; injected closing rollback rejected; unmodified tree still passes |
| `python3 scripts/check_writer_disposition.py` | `writer-disposition check ok: 39 declared roots, 17 writer tools, 20 writer modules` |
| `python3 scripts/check_writer_disposition.py --self-test` | `writer-disposition self-test ok` |
| `cargo clippy --workspace --all-targets -- -D warnings` | **passed** (after the five pre-existing branch lints in §4.2 were fixed) |
| `scripts/verify.sh quick` | **passed** (`Quick verification passed.`) — agent schema, core/client/desktop boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, queue-delivery-callsites, queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint, Eggwork target routing, writer-disposition (with self-test), first-party skills, shell-policy test safety, and `cargo check --workspace --all-targets --locked` |

These are **local** runs. No CI run was made for this change.

### 4.1 The root integration suite

`cargo test --test writer_permit_service` — **12 passed**, 0 failed:

```
test a_principal_outside_the_project_is_refused ... ok
test the_lease_owner_may_acquire ... ok
test a_run_without_the_lease_is_refused ... ok
test unproven_quiescence_parks_the_permit ... ok
test the_operator_can_inspect_and_release_a_parked_permit ... ok
test the_operator_release_needs_proven_quiescence ... ok
test distinct_checkouts_do_not_wait_on_each_other ... ok
test startup_reconciliation_parks_a_permit_left_live ... ok
test a_dropped_guard_quarantines_rather_than_frees ... ok
test a_waiter_takes_the_permit_when_it_is_released ... ok
test a_cancelled_wait_gives_up_without_taking_the_permit ... ok
test a_timed_out_wait_gives_up_without_taking_the_permit ... ok

test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

### 4.2 Pre-existing branch defects fixed in passing

`cargo clippy --workspace --all-targets -- -D warnings` **failed on this
branch before C002's code was clippy-checked**, on four lints that C002 did not
introduce:

| Site | Lint | Origin |
|---|---|---|
| `crates/codegg-core/src/workspace_services.rs:156` | `unnecessary_sort_by` | the branch's own `ac15b25e` ("Implement agent tool-use operability corrections"), which is **not** an ancestor of `origin/plan/agent-workstream-m005-writer-audit` |
| `src/agent/tool_batch.rs:137` | `empty_line_after_doc_comments` | same commit |
| `src/mcp/mod.rs:554` | `collapsible_if` | the branch's Eggsearch host-scope work |
| `src/search_backend/eggsearch.rs:626` | `too_many_arguments` | same |
| `tests/agent_loop_harness.rs:1049` | `redundant_locals` | the branch's agent-loop harness work |

None is in a file C002 touched, and all five are mechanical. They are fixed
here in a **separate commit** so the branch can pass the clippy gate that
`verify.sh full` and CI both run, and so C002's own commit stays free of
unrelated edits. The `too_many_arguments` site uses the `#[allow]` the repo
already uses at five other call sites; the other three are behaviour-preserving
rewrites. The `redundant_locals` pair was a genuinely pointless
`let output_a = output_a;` rebinding and is simply deleted.

C002's own clippy finding was one: `#[async_trait]` on
`WriterPermitAuthority` trips `double_must_use` on Rust ≥ 1.99, because the
expansion injects a `#[must_use]` onto a return type that is already
`must_use`. It is suppressed at the macro site with the same attribute and the
same explanation the repo already uses for `crate::core::CoreClient`.

### 4.3 A pre-existing environmental failure, not caused by this change

`cargo test --test agent_loop_harness` reports **3 failures** on this machine
in its default configuration:

- `admitted_native_command_timeout_preserves_partial_output_in_agent_transcript`
- `shell_escalation_ask_waits_for_explicit_approval_before_dispatch`
- `verify_and_two_explicit_in_progress_todos_reach_the_model_through_agent_loop`

All three fail with the same line:

```
sandbox helper failed: managed scratch path is owned by a different user: [HOME]/.cache
```

The cause is the **local machine's** `~/.cache` mode, not the code.
`src/scratch.rs:474` refuses a cache root that is owned by another user **or**
is group/world-writable:

```rust
if metadata.uid() != expected_uid.uid() || metadata.mode() & 0o022 != 0 {
    return Err(ScratchError::WrongOwner(path.to_path_buf()));
}
```

`~/.cache` here is owned by `sugarwookie` but is mode **777**, so
`0o777 & 0o022 != 0` and the helper refuses it. The check is right: a
world-writable cache directory is exactly the condition it defends against.

**Proof that the code is fine.** The same suite, with the cache root pointed at
a private directory, passes in full:

```
$ CACHE=$(mktemp -d); chmod 700 "$CACHE"
$ XDG_CACHE_HOME="$CACHE" cargo test --test agent_loop_harness
test result: ok. 67 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
```

The one-line remedy on this machine is `chmod 700 ~/.cache`. It was **not**
applied as part of this change: it is a change to the user's home directory,
not to the repository, and it is not C002's business. The three failures are
pre-existing and environmental; they are recorded here rather than hidden, and
they are not in `scripts/verify.sh quick` (which runs `cargo check`, not the
test suites).

### 4.4 A pre-existing guard failure, not caused by this change

`python3 scripts/check_policy_surface.py` **fails** on this branch:

```
check_policy_surface: FAILED
 - tool_batch.rs: Always-choice persist must derive the capability scope
```

The rule requires `decision_scope_for_tool_call` in `src/agent/tool_batch.rs`.
That identifier occurs **zero** times in both this worktree and
`origin/plan/agent-workstream-m005-writer-audit` (the branch this worktree
rebased onto), and C002 changed no file under `src/agent/`. The failure is
therefore inherited from the target branch, not introduced here.

It is recorded rather than hidden, and it is **not** a C002 blocker:
`check_policy_surface.py` is a change-triggered guard and is **not** in
`scripts/verify.sh quick` nor in CI. The two files it names are the
agent-tool-use operability line's, not the writer-containment line's.
## 5. Invariant review

| Invariant | Status |
|---|---|
| The permit is the **only** answer to "who may write to this checkout" | Held. One row per checkout, one durable store, one CAS. There is no second in-memory permit table. |
| The worktree **owner** is distinct from the permit **owner** | Held. The lease lives in `managed_worktree`/`worktree_lease`; the permit lives in `writer_permit`. The permit's owner is the writer; the lease's owner is the run. |
| A permit is never freed without proven quiescence | Held. `complete_release` has no path that frees without evidence, and the operator path is evidence-gated the same way. |
| A lease deadline never frees a live writer | Held. The store contains no expiry-driven release path at all. |
| A restart never frees a permit | Held. Restart reconciliation quarantines every live permit. |
| A dropped or panicked holder never frees a permit | Held. `WriterPermitGuard::drop` quarantines; the store leaves a panicked holder's permit **held** rather than freed. |
| The permit is not a scheduler resource | Held. The scheduler's exclusivity keys are empty for every writer-bearing job kind; the permit has its own store and its own waiter. `scripts/check_scheduler_bypass.py` passes. |
| The permit is not a model tool and exposes no bearer token to a model | Held. It is not registered in the tool catalog, has no `CoreRequest` variant, and its possession token is never projected. |
| `codegg-core` stays UI/server/tool-free | Held. `scripts/check-core-boundary.sh` passes. The new module imports only `codegg-core` internals plus `sqlx`, `serde`, `thiserror`, `chrono` and `uuid`. |
| No shared mutation is enabled | Held. No writer consults the permit; no selector, flag, config key or protocol surface exists. |
| The existing isolated worktree behaviour is unchanged | Held. No `src/tool/**` file changed, and no default path was altered. |

## 6. Failure and recovery review

| Failure | Behaviour | Evidence |
|---|---|---|
| Two writers race for one checkout | One wins; the other is refused with `Held` and may wait | `two_acquirers_produce_exactly_one_holder`; `sixteen_concurrent_acquirers_produce_exactly_one_holder` |
| A waiter is cancelled | It gives up holding nothing | `a_cancelled_wait_gives_up_without_taking_the_permit` |
| A waiter times out | It gives up holding nothing | `a_timed_out_wait_gives_up_without_taking_the_permit` |
| A holder panics | The permit stays **held**; the store never half-writes a row | `a_panicking_holder_leaves_the_permit_held` |
| A holder's transaction is interrupted | The transaction rolls back; no half-written owner | `an_uncommitted_transaction_rolls_back` |
| A holder is abandoned (guard dropped) | The permit is **quarantined**, not freed | `a_dropped_guard_quarantines_rather_than_frees` |
| The holder cannot prove its effects settled | The permit is quarantined with the caller's reason | `unproven_quiescence_quarantines_instead_of_releasing` |
| The daemon restarts with a live permit | The permit is quarantined as `DaemonRestart` | `restart_reconciliation_quarantines_a_live_permit`; `startup_reconciliation_parks_a_permit_left_live` |
| The daemon restarts mid-release | The `Releasing` permit is quarantined | `restart_reconciliation_quarantines_an_interrupted_release` |
| The checkout was re-leased mid-write | The new generation's holder is refused (`StaleLeaseGeneration`); the permit stays with its holder | `a_permit_held_under_an_old_generation_blocks_a_new_generation` |
| The same acquisition is retried | The same permit is returned; an altered payload is a conflict | `an_idempotent_retry_returns_the_same_permit`; `an_idempotent_retry_with_an_altered_payload_is_refused` |
| A quarantined permit blocks writers | It is not acquirable until an operator releases it | `a_quarantined_permit_needs_an_operator_release` |

Recovery is always **explicit**: a quarantined permit is released by an
operator with proven quiescence. There is no automatic path out of quarantine.

## 7. Migration and compatibility review

- `migrate_v84` is **additive**: it creates one new table and three indexes.
  No existing table is altered, no stored data is rewritten, and no column is
  dropped.
- `STORAGE_LAYOUT_VERSION` moved 83 → 84, and
  `scripts/check_project_catalog_invariants.py` confirms the marker still
  tracks the highest wired schema migration.
- The migration is idempotent (`CREATE TABLE IF NOT EXISTS`, `CREATE INDEX IF
  NOT EXISTS`), so a re-run against a partially migrated database is safe.
- A database that predates v84 simply has no permit rows; nothing reads them
  until a permit is first taken.
- The `user_prompt_queue` refactor changed **no** stored shape. The queue
  store's transaction shape is byte-for-byte the same shape it was guarded
  for; only the type's location moved.
- The legacy isolated worktree placement is untouched. `src/tool/**` is
  unchanged.

## 8. Security review

- **The scope is never model-supplied.** `WorktreePermitAuthority` resolves
  the project, repository, workspace, worktree, lease generation, group, and
  principal from daemon-owned stores. A request payload cannot widen it.
- **The possession token is not a bearer token to a model.** It is minted by
  the store, is never projected, and is required on every mutation. It is
  *not* an authorization credential: the authority check happens before the
  token exists.
- **The possession token is non-duplicable.** A fresh UUID per acquisition, and
  a transfer replaces it in the same statement that invalidates the old one.
- **Cross-tenant denial fails at the authority.** Only the run that holds the
  checkout's lease may take a permit, and a run in another project cannot hold
  this checkout's lease. The `file.modify` capability is additionally checked
  through the daemon's authorization service when one is configured, so a
  viewer-role principal is refused.
- **The permit leaks nothing.** The operator view carries identity and state
  only — no path, no command, no free text, no token. The operation is a
  closed enum and the quarantine detail is bounded and truncated on write.
- **A quarantined permit cannot be silently released.** Only an operator path
  with proven quiescence can, and it never touches the checkout.
- **The permit is not a sandbox and not an exclusion.** It coordinates
  CodeGG's own writers only. An unrelated process running as the same user is
  outside its guarantee, exactly as ADR-0042 clause 1 requires. The user-facing
  label stays "CodeGG-mediated serialized checkout".

## 9. Documentation and operations

- `architecture/writer_permit.md` — the module contract: the state machine,
  the fence, idempotency, quiescence, the holder contract, the waiter, restart
  reconciliation, authorization, redaction, and an explicit "what this does not
  claim" section.
- `architecture/overview.md` — module-map row, the `codegg-core` module list,
  the deep-dive index, and the re-measured counts (architecture docs 88,
  `codegg-core` modules 48, database tables 93, storage layout version 84,
  integration test files 267).
- `.opencode/skills/scheduler/SKILL.md` — a "the checkout writer permit is NOT
  a scheduler permit" section, so the next person does not reach for an
  exclusivity key.
- `AGENTS.md` — a gotcha naming the permit as a third authority (distinct from
  the worktree lease and from the scheduler), with the two rules that are easy
  to get wrong: no release without positive quiescence evidence, and a dropped
  guard quarantines rather than frees.
- Operator surface: `WriterPermitService::{inspect, list, operator_release,
  operator_quarantine}`. It is a daemon service, not a CLI command and not a
  protocol route — C002 adds no user-visible surface, so the "no rollout
  before C006" rule is respected.
- Startup reconciliation logs a warning naming the number of permits it
  parked.

## 10. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| medium | The daemon's startup reconciliation only quarantines permits it can see in the `writer_permit` table. A permit row whose *checkout* has since been removed is still quarantined, but the operator view will show a checkout that no longer exists. | Accepted for C002. The permit is parked, which is the safe direction; a later milestone that removes worktrees should decide whether to also drop their permit rows. |
| low | The operator surface is a daemon service with no CLI or protocol route, so an operator must use the daemon API to release a quarantined permit. | Accepted for C002. C002 adds no user-visible surface by design; a route is a C004/C006 concern. |
| low | `WriterPermitGuard::drop` needs a Tokio runtime handle to spawn its quarantine. Without one, the permit stays held until the next daemon restart reconciles it. | Accepted. Both outcomes are fail-safe (parked, never freed). |
| low | The permit's idempotency binding is scoped to the live acquisition; a key is released when the permit is released or quarantined and may be reused afterwards. | Accepted and documented in `architecture/writer_permit.md`. |

No critical or high finding remains. Nothing here blocks C003.

## 11. Roadmap disposition

- **C002: closed.** The permit's authority, its lifecycle service, its daemon
  consumer, its waiter, its operator surface, and its qualification evidence
  all exist.
- **C003 (native file/Git writer interposition, Q2): unblocked** by this
  closure, subject to its own predecessor check. C003's plan header now reads
  "accepted ADR-0042 (option B, 2026-10-19)". C003 remains *not started*.
- **C004, C005, C006: still blocked**, on their own ordered predecessors.
- **Parent M005: still not closed.** C001–C006 and the Q1–Q5 evidence are all
  still required, and C006 must produce a real production path.
- **Independent M006/M007: unaffected.** C002 changed no isolated-worktree
  behaviour.

## 12. Registry updates

- `plans/registry.md`:
  - the M005 corrective row now reads "C002 closed; C003 ready; C004–C006
    blocked; shared-write unavailable";
  - the C002 plan moved from *Active implementation plans* to *Recently
    closed* with this record;
  - the C003 row moved from *Blocked work* to *Dependency-ready implementation
    plans* — this is the unblock check the planning process requires;
  - the C002 row was removed from *Blocked work*.
- `plans/subsystems/agent-workstream-blackboard-m005-writer-containment-corrective-addendum.md`:
  status, dependency graph, milestone table and the C002–C006 gate notes.
- `plans/implementation/agent-workstream-m005-writer-containment-corrective/002-durable-writer-permit-and-quarantine.md`:
  status → closed, with a "Delivered by" section.

