# User Prompt Queue Corrective C002 — Closure Record

Status: **closed**
Date: 2026-10-18
Class: **invariant**
Plan: `plans/implementation/user-prompt-queue/C002-queue-write-transaction-shape-corrective.md`
Roadmap: `plans/subsystems/user-prompt-queue-roadmap.md#7-milestones` (Corrective C002)
Baseline: `2acce2138` (M007 closed, branch in sync with `origin/main`).

**No migration.** `STORAGE_LAYOUT_VERSION` stays **82**; v80, v81, and v82 are
untouched. This corrective changes how the store *opens a transaction*, never what
it stores.

M003's, M006's, and M007's closure records are **not** rewritten. Where each of
them recorded this defect, it carries a dated annotation pointing here.

## 1. What shipped

Every queue path that **reads before it writes** now opens its transaction with
`BEGIN IMMEDIATE` instead of a deferred `BEGIN`.

```rust
// crates/codegg-core/src/session/user_prompt_queue.rs
async fn begin_immediate(pool: &SqlitePool) -> Result<ImmediateWrite, UserPromptQueueError> {
    ImmediateWrite::begin(pool).await
}
```

`BEGIN IMMEDIATE` acquires the write lock **before** any read, and that acquisition
*does* honour `busy_timeout`. A second writer waits for the first to commit and then
proceeds — which is the behaviour every one of these paths already assumed.

## 2. The defect, precisely

`SqlitePool::begin()` issues a deferred `BEGIN`, which takes **no lock**. The first
`SELECT` then holds a shared read lock, and the first `UPDATE` has to *upgrade* it
to a write lock. SQLite refuses that upgrade with `SQLITE_BUSY` — code 6,
`database is deadlocked` — and deliberately does **not** invoke the busy handler,
because waiting for a peer to release a read lock could itself deadlock.

`busy_timeout` therefore never applies to the upgrade. Two callers that each read
before either wrote could **both** fail, instead of one winning and the other
waiting its turn.

**What was never at risk:** the at-most-once fence. Neither caller wrote anything,
so nothing was double-delivered. What was at risk was a delivery attempt failing
for a reason the daemon cannot act on — and M003's own test failing on it, which is
how this surfaced at all.

## 3. Measured, before and after

The pre-existing test was one race per run, and it failed roughly 4 runs in 12 — a
rate too low to act on and too high to ignore. The corrective added a **repeated**
harness so the number is a measurement rather than an anecdote.

| | Rounds | Rounds with a storage failure | Rate |
|---|---|---|---|
| Before | 256 (4 runs x 64) | 204 | **79.7%** |
| After | 192 (3 runs x 64) | 0 | **0%** |

Both directions assert the same two things per round: **no caller fails to reach the
store**, and **exactly one** caller wins the entry. The second half matters as much
as the first — serializing the path must not have weakened the fence, and it did
not. One failing side is enough to fail a round, so the "before" column counts
rounds where *at least one* caller received `SQLITE_BUSY`.

## 4. Scope

Converted (all eight read-then-write paths): `add`, `edit`, `delete`, `reorder`,
`adopt`, `transfer`, `claim_next_for_delivery`, `mark_steer_delivered`.

Deliberately left deferred, each for a stated reason that the guard enforces:

- `ownership` — writes nothing at all, so there is no read lock to upgrade.
- `list_live` — *does* write, via the lazy expiry sweep, but its **first** statement
  is that sweep, which already takes the write lock there. Taking it earlier would
  serialize every queue read in the daemon and buy nothing.
- `mark_delivered`, `mark_delivery_failed`, `release_delivery`, `retry_failed` — one
  guarded `UPDATE` each, with no prior read and therefore no upgrade to refuse.

## 5. Three things that were wrong on the way, and what fixed them

**The rollback could not close the connection.** The first implementation rolled
back an uncommitted transaction by dropping the pooled connection. That works for a
file-backed database and is **destructive** for a shared-cache in-memory one, where
dropping the last connection destroys the database. The mutation suite caught it as
`user_prompt_queue_unowned_queue_is_refused_for_every_mutation` failing on `reorder`
with `user_prompt_queue_storage_error` — two operations after the refusal, with
`no such table` underneath. The drop now queues a rollback on the connection instead,
which is what `sqlx::Transaction` does on its own drop.

**Raw SQL silently broke sqlx's transaction bookkeeping.** The first fix issued
`sqlx::query("BEGIN IMMEDIATE")` directly. sqlx tracks transaction **depth** per
connection, and only `Command::Begin` increments it — so a raw `BEGIN` left the
counter at zero, the queued rollback became a **no-op**, and connections went back to
the pool still holding their write lock. The symptom was a suite that hung rather
than failed. The fix goes through `sqlx`'s `TransactionManager::begin`, which takes
the statement to run; `commit` and `start_rollback` then keep depth correct.

**A deterministic reproduction hangs, so it was removed.** A hand-written test that
opened two deferred transactions, read on both, and then tried to write was written
to pin the mechanism exactly. It hung: sqlx's SQLite driver resolves lock waits
through an *unlock-notify* VFS, so a connection parked on a lock its peer will never
release waits indefinitely rather than returning `SQLITE_BUSY`. A test that hangs
the binary is a worse failure than the one it would have proved, so it was deleted
and the reasoning kept as a comment on the race test. The *shape* is pinned
statically instead (§6), where it cannot hang.

## 6. The guard

`scripts/check_queue_write_transaction_shape.py` fails when a function that writes
queue tables opens a deferred transaction, when a deferred site is not on the
allowlist, when an allowlist entry no longer matches a deferred site, when
`begin_immediate` stops issuing `BEGIN IMMEDIATE`, or when the drop stops rolling
back / starts closing.

`--self-test` proves it is load-bearing: it injects a deferred read-then-write path
and a closing rollback, requires both to be rejected, and then requires the
unmodified tree to still pass. Wired into `scripts/verify.sh quick` and into CI
alongside its sibling `check_queue_delivery_callsites.py`.

## 7. Records annotated, not rewritten

- `plans/closure/user-prompt-queue/003-status.md` — the claim test keeps its original
  assertion and its original wording; the test's doc comment now says the cause is
  fixed and names C002.
- `plans/closure/user-prompt-queue/006-status.md` — annotated at the paragraph that
  records the flake rate.
- `plans/closure/user-prompt-queue/007-status.md` §10 — annotated, keeping the
  decision to defer as M007's to record.

**One citation drift found and not corrected in place:** M007's §10 says the defect
was reported in `006-status.md` §7. M006 §7 records four findings and this is not
among them; M006 recorded the *rate* at line 118 and the finding itself lives in the
roadmap's §10. The M007 text is left alone — it is a historical record — and the
authoritative statement is the roadmap row, which C002 discharges.

## 8. What this does not do

- It does not change the at-most-once fence. `claim_next_for_delivery` still selects
  a candidate and re-guards the `UPDATE` on `status = 'pending'`; it simply no longer
  fails to get there.
- It does not make delivery reliable across *processes*. `BEGIN IMMEDIATE` serializes
  writers across every connection, which is strictly more than a process-local lock
  would have done, but it is not a distributed lock and the daemon remains a
  user-scoped singleton by `flock`.
- It does not touch the other open items: `retry_failed`'s revive position and the
  second-displacement collision, both C001 §10 and still open maintainer decisions.
- It does not claim the whole workspace is free of deferred read-then-write
  transactions. It constrains the queue store, which is where the recorded defect
  lives; other stores were not audited, and no claim is made about them.
## 9. Verification

Run locally against the working tree. **No hosted CI claim is made or implied**;
the CI job in `.github/workflows/ci.yml` now runs the new guard, but this record
reports what was actually run here.

| Command | Result |
|---|---|
| `cargo fmt --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean, no warnings |
| `cargo test -p codegg-core --test user_prompt_queue` | 22 passed, 0 failed |
| `cargo test -p codegg-core --test user_prompt_queue_delivery` | 39 passed, 0 failed |
| `cargo test -p codegg-core --test user_prompt_queue_mutation` | 34 passed, 0 failed |
| `cargo test --test user_prompt_queue_box` | 64 passed, 0 failed |
| `cargo test --test storage_migrations` | 5 passed, 0 failed |
| `cargo test --test identity_m003_daemon_authorization` | 27 passed, 0 failed |
| `check_queue_write_transaction_shape` | pass |
| `check_queue_write_transaction_shape --self-test` | pass — deferred path and closing rollback both rejected |
| `check_queue_delivery_callsites` | pass |
| `check_queue_delivery_callsites --self-test` | pass — fourth caller rejected |
| `check_project_catalog_invariants` | pass |
| `check_adr_integrity` | pass |
| `check_audit_coverage` | pass |
| `check_http_route_disposition` | pass |
| `check_scheduler_bypass` | pass |
| `check-core-boundary` | pass |
| `scripts/verify.sh quick` | pass |

`scripts/verify.sh quick` runs the new guard as one of its steps
(`scripts/verify.sh:107`), and the script is `set -euo pipefail`, so a green
`quick` here includes it.

`scripts/verify.sh quick` does not run workspace tests, so the pre-existing
macOS `/tmp`-symlink failure family recorded in M007 §7b is untouched by this
corrective and was not re-measured here.
