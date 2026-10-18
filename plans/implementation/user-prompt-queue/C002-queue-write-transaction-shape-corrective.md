# User Prompt Queue Corrective C002 — Queue Write Transactions Cannot Both Fail

Status: **closed** — implemented 2026-10-18. Closure record:
`plans/closure/user-prompt-queue/C002-status.md`.

Class: **invariant**. **No migration** — `STORAGE_LAYOUT_VERSION` stays **82** and
v80/v81/v82 are untouched. This corrective changes how the store opens a
transaction, never what it stores.

Source roadmap:
`plans/subsystems/user-prompt-queue-roadmap.md#10-risks-and-decision-points` (the
"two concurrent queue write transactions can both fail" row) and
`plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Corrective C002).

Origin: **a defect found by M006 while repeating a suite, carried forward
deliberately.** M007's closure record named it as *not fixed* on the maintainer's
instruction — land M007, file the corrective — and this is that corrective. M003's
and M006's records are **not** rewritten; this plan corrects the store they
described.

Related records this closes out:

- `plans/implementation/user-prompt-queue/003-queue-delivery-through-turn-admission.md`
  / `plans/closure/user-prompt-queue/003-status.md` — shipped the delivery claim
  whose test was annotated as flaky.
- `plans/closure/user-prompt-queue/006-status.md` §7 — found and reported it.
- `plans/closure/user-prompt-queue/007-status.md` §10 — deliberately carried it
  forward rather than widening M007.

Applicable ADRs: none. This is a transaction-shape invariant inside one store, not
an authority or interface decision; the roadmap row recorded the decision point and
`architecture/session.md` now states the contract.

## 1. The defect

`SqlitePool::begin()` issues a **deferred** `BEGIN`, which takes no lock. The first
`SELECT` in that transaction then holds a shared read lock, and the first `UPDATE`
has to *upgrade* it to a write lock. SQLite refuses that upgrade with
`SQLITE_BUSY` — code 6, `database is deadlocked` — and deliberately does **not**
invoke the busy handler, because waiting for the other connection to release a
read lock could itself deadlock.

So `busy_timeout` never applies to the upgrade. Two callers that each read before
either wrote could **both** fail, instead of one winning and the other waiting its
turn. Every queue write path had that shape: `add`, `edit`, `delete`, `reorder`,
`adopt`, `transfer`, `claim_next_for_delivery`, and `mark_steer_delivered`.

The **at-most-once fence was never at risk** — neither caller wrote anything, so
nothing was double-delivered — but a delivery attempt could fail for a reason the
daemon cannot act on, and M003's own test failed on it at roughly 4 runs in 12.

## 2. The decision

Take the write lock **before** any read: `BEGIN IMMEDIATE`.

- It acquires RESERVED up front, which **does** honour `busy_timeout`. A second
  writer waits for the first to commit and then proceeds.
- It is the repository's existing precedent: `crates/codegg-core/src/workstream_claim.rs`
  already opens its write paths this way.
- Holding the lock for the whole transaction is the point, not a cost. Each path
  reads a row, decides from what it read, and writes a decision that must not be
  invalidated in between; serializing the path closes exactly that window.

Rejected: a **single-statement fence** (one `UPDATE … WHERE id = (SELECT …)`) would
remove the read-then-write window for the claim alone, but it cannot span the
ownership check on the other table, and it fixes one path rather than the shape.
Rejected: a **process-local mutex** over the write paths. It would serialize
writers inside one process but does nothing across processes, and the repo has
already been bitten by a store-level lock that self-deadlocks
(`run_store.rs`, on re-entrant acquisition).

## 3. Scope — what is and is not converted

Converted to `begin_immediate` (all eight read-then-write paths): `add`, `edit`,
`delete`, `reorder`, `adopt`, `transfer`, `claim_next_for_delivery`,
`mark_steer_delivered`.

Deliberately left on a deferred transaction:

- `ownership` — writes nothing, so there is no read lock to upgrade.
- `list_live` — does write (the lazy expiry sweep), but its **first** statement is
  that sweep, so it already takes the write lock at that statement. Taking it
  earlier would serialize every queue read in the daemon for nothing.
- `mark_delivered`, `mark_delivery_failed`, `release_delivery`, `retry_failed` —
  each is a single guarded `UPDATE` with no prior read, so there is no upgrade to
  refuse.

## 4. Work packages

- **A** — `ImmediateWrite`: acquire a connection, `BEGIN IMMEDIATE`, run the body,
  commit; roll back on drop. It must go through sqlx's `TransactionManager`, and
  its drop must queue a rollback rather than close the connection.
- **B** — convert the eight paths and the nine shared helpers.
- **C** — measure before and after, and land a test whose failure rate is a
  measurement rather than an anecdote.
- **D** — pin the shape with `scripts/check_queue_write_transaction_shape.py`,
  wired into `scripts/verify.sh quick` and CI, with a self-test that proves it
  rejects an injected violation.

## 5. Acceptance

1. Zero storage failures across 64 repeated concurrent claim rounds, where the
   same harness failed **204 of 256** rounds before the change.
2. Every round still admits **exactly one** entry. The at-most-once fence is
   strengthened, never weakened.
3. All queue store suites green: `user_prompt_queue`, `user_prompt_queue_delivery`,
   `user_prompt_queue_mutation`, `user_prompt_queue_box`.
4. The guard passes, and `--self-test` proves it rejects both an injected deferred
   read-then-write path and an injected closing rollback.
5. `architecture/session.md`, `AGENTS.md`, the roadmap, and `plans/registry.md`
   describe the delivered contract. **No migration.**