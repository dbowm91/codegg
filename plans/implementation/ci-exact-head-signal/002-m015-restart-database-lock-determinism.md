# Exact-HEAD CI E002 — M015 Corrupt-Journal Restart and SQLite Lock Determinism

Status: ready
Reviewed baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`, 2026-10-26. Original failure: `CI / verify` run `40224670033`, SHA `bec31b1e5a`, 2026-10-25.
Source: `plans/subsystems/ci-exact-head-signal-corrective-addendum.md`; original Tool Programs M015 `plans/implementation/tool-programs/015-final-production-path-and-independent-closure.md`. Primary class: invariant/infrastructure; no new ADR unless store ownership/recovery authority changes.

## 1. Objective

Determine whether the real daemon has a job-recovery SQLite lock defect or the Tool Programs M015 crash/restart test leaves a writer/daemon in flight. Repair the **actual owner** and make the negative corruption test deterministically prove its intended invariant: a corrupt checkpoint journal is rejected on restart, not silently treated as empty/recovered and not spuriously masked by database contention.

## 2. Evidence

GitHub `https://github.com/dbowm91/codegg/actions/runs/40224670033` shows:
- `tests/tool_programs/tool_program_m015_daemon_failpoints.inc:corrupt_checkpoint_journal_is_rejected_after_restart` failed after ~60.5 s in the hosted test family.
- Native log: `job recovery failed; attempts from a prior daemon generation may not be recovered`, with `storage failure: database error: (code: 5) database is locked`.
- `src/core/daemon_bootstrap.rs` logged the recovery failure. This test was supposed to assert corruption rejection; a busy database instead prevents reaching the checked recovery path.
- The newer 2026-10-26 CI `40324996955` fails earlier at Clippy, so it provides **no** evidence that restart locking is fixed. E001 owns that lint blocker.

Neither increasing an arbitrary timeout nor `if let Err(_) ... { return Ok(0) }` is acceptable.

## 3. Invariants and non-goals

- Restart cannot report zero recovered attempts in place of a database error; corrupted persisted journal must be rejected with a typed, independently asserted cause and no jobs dispatched.
- Each daemon and test generation owns SQLite connections, lock files, child processes and event subscribers; previous generation's store must be fully quiescent before new generation's recovery preflight unless the product contract intentionally permits concurrent access, in which case contention must be a valid typed outcome.
- Test corruption must be introduced *only after* durable checkpoint/journal write completion, not while a writer still owns an in-flight transaction.
- Default CI need not include a paid provider or a configured external Eggwork node. No weakening of Tool Programs broker authorization, idempotency, durable replay or security guards.
- Race reproduction must distinguish `SQLITE_BUSY` and `SQLITE_LOCKED` from corrupt data; no silent cleanup or retries that change assertions.
- Avoid test-wide serialization, full repository CI redesign or nonstandard test-only behavior unless source-level contention makes it necessary and narrowly proven.

## 4. Dependencies

E002 independently ready, even while E001 blocks hosted tests. Use local focused test to debug and wait for E001's Clippy cleanup for combined green `CI / verify`. Tool Programs is the code owner for recovery behavior; coordinate with native storage/job store for actual fix. Existing branch/CI stabilization roadmap remains closed.

## 5. Ordered work packages

A. **Failure reproduction:** inspect the exact test fixture, daemon shutdown/failpoint sequence, journal writer, `src/core/daemon_bootstrap.rs` job recovery and SQLite pool/connection cleanup. Compare local isolated `cargo test -p codegg --test tool_programs ...` and nextest/default hosted parallelism. Instrument PID/generation, WAL checkpoint, transaction-close and test-root lineage without leaking workspace/provider data.

B. **Deterministic schedules:** reproduce against (i) sequential one test, (ii) same target under parallel load, (iii) whole CI family test order, (iv) two daemon generations with deliberate source-side late writer. Add bounded barrier/failpoint synchronization so the negative journal can be corrupted **after** writer durability and **before** next daemon recovery. Demonstrate forced SQLite busy remains an explicit error, and an actual corrupt journal exercises its own negative path.

C. **Minimal owner repair:** if fixture teardown is wrong, await/terminate only owned old daemon and close pools before re-opening; use distinct per-test state directories and no shared default database. If production recovery holds a connection incorrectly, adjust transaction lifetime/retry semantics through canonical storage service with bounded behavior. Do not globally increase SQLite busy timeout as sole solution or swallow busy errors. Recheck concurrent session/job store users.

D. **Repeatability:** rerun failure case enough times under controlled high contention to reveal a genuine sporadic regression and record exact denominator, env/toolchain, child cleanup and each result; don't claim “unflaky” from a single local pass. Prove job recovery refuses corrupted checkpoint with correct typed evidence and that idempotent valid recovery is unaffected.

E. **CI reconciliation:** after E001, run exact-HEAD strict Clippy and full `CI / verify`. Do not merge a green-test-label change if the test is skipped; verify the test name appears in the executed-workload summary. Route any new unrelated failing tests to their owner with exact paths/logs.

## 6. Failure, cancellation, crash/restart, concurrency

SIGKILL during an actual transaction may leave recoverable WAL; the recovery routine must distinguish legal interrupted transaction (SQLite rollback/recovery) from structurally corrupt persisted checkpoint. Two independently owned daemons may not silently share mutable data home; an old process must not be killed by unverified same-name PID. A busy error is retryable *only* within documented bounded ownership/recovery policy, and exhausted contention fails closed and leaves the store untouched.

## 7. Migration and compatibility

No production schema migration expected. If recovery-service transactional semantics must change, document WAL/PRAGMA/migration/compatibility and validate against existing production DB. CI fixture changes must preserve the source negative invariant and correct test coverage.

## 8. Tests

```bash
cargo test -p codegg --test tool_programs -- tool_program_m015_daemon_failpoints::corrupt_checkpoint_journal_is_rejected_after_restart --nocapture
cargo test -p codegg --test tool_programs -- tool_program_m015_daemon_failpoints
cargo test -p codegg --test tool_programs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```
Adapt exact test module filtering to current target layout. Run repeat-count/parallelism matrix separately; do not use a harness watchdog that kills an unrelated daemon. Record hosted whole CI, not only focused tests. Tests must prove bad journal rejection and valid restart, plus a forced-lock **typed failure** control.

## 9. Acceptance / stop / closure

Accept only after deterministic corruption/recovery assertions under repeated runs, normal valid restart, real lock contention typed as lock (not “journal corrupt”), no orphan writer/daemon, exact-HEAD hosted CI full suite green after E001 and unchanged Tool Programs safety contract.

Stop if ownership/migration architecture cannot be resolved in this narrowly scoped corrective; raise a new ADR/plan, retaining E002 blocked. Closure `plans/closure/ci-exact-head-signal/002-status.md`: source and run IDs, timeline, root cause, distinction fixture/product, negative/positive tests and counts, DB cleanup/security, exact-head CI, and final registry/roadmap disposition.
