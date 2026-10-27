# Exact-HEAD CI Signal — October 26 Corrective Addendum

Status: E001 and E002 closed on branch head `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`; exact-head CI run 40383901325 passed. The earlier main baseline failures below remain historical evidence.
Reviewed baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`, 2026-10-26. Refresh GitHub Actions statuses on entry.
Related already closed predecessor: `plans/subsystems/branch-and-ci-stabilization-roadmap.md` C001. This is a **new incident**, not reopening or rewriting its valid historical closure.
Related work: `plans/subsystems/harness-lab-continuation-corrective-addendum.md` (operational green exact-head gate).
Normative: `plans/000-long-term-specification.md` correctness/security/recovery, `plans/003-planning-process.md`, `architecture/testing.md`.

## 1. Purpose/ownership

Restore **trustworthy mainline CI**, separating two real failure classes: newly surfaced all-targets Clippy diagnostics and the previous tool-program restart test's SQLite lock/timeout. These require different owners/tests; neither should be “fixed” by weakening `-D warnings`, ignoring failed tests, isolating CI broadly, or suppressing database errors.

## 2. Verified findings

**E001 — Exact-head Clippy failure:** `https://github.com/dbowm91/codegg/actions/runs/40324996955` (SHA `957fcbf00e17`) failed `Workspace Clippy` with Rust/Clippy 1.99.0, `cargo clippy --workspace --all-targets --locked -- -D warnings`. Twelve lib/test diagnostics across `src/eggsact/adapter.rs`, `src/search_backend/eggsearch.rs`, `framing.rs`, `src/test_runner/index.rs`, `runner.rs`, `src/tool/deterministic.rs`, `edit.rs`, `task.rs`, `skill.rs`. Harness Lab Python tests and preceding guards passed. This is a *source lint gate*, not proof of a runtime Harness Lab failure.

**E002 — Restart-test lock failure:** `https://github.com/dbowm91/codegg/actions/runs/40224670033` (SHA `bec31b1e5a`) failed after ~60 s in `tool_program_m015_daemon_failpoints::corrupt_checkpoint_journal_is_rejected_after_restart` (file `tests/tool_programs/tool_program_m015_daemon_failpoints.inc`). Native log: job recovery failed with `database is locked` (SQLite code 5). Root cause not established: could be retained writer connection, lock lifetime, async teardown or test contention. The Clippy failure on latest head occurs **earlier** in the workflow and therefore does **not** prove the SQLite defect is fixed.

## 3. Work classification and boundaries

Primary infrastructure/invariant: exact-head deterministic CI. Existing `branch-and-ci-stabilization` predecessor remains closed. M015 tool-program journal and job recovery semantics are owned by Tool Programs/runtime: E002 is a **test-qualifying corrective**, not a second job-scheduler architecture. Do not turn SQLite lock errors into empty recovery success. CI gates only qualify executed tests on precise SHA.

## 4. Dependencies / work items

| Plan | Status | Hard dependency |
|---|---|---|
| E001 — all-targets Clippy exact-head restoration `plans/implementation/ci-exact-head-signal/001-clippy-gate-exact-head-restore.md` | closed | `plans/closure/ci-exact-head-signal/001-status.md`; exact-head CI 40383901325 passed |
| E002 — Tool Programs M015 crash/restart SQLite lock determinism `plans/implementation/ci-exact-head-signal/002-m015-restart-database-lock-determinism.md` | closed | `plans/closure/ci-exact-head-signal/002-status.md`; exact-head CI 40383901325 passed |

E001 and E002 were implemented separately; **final combined verification** is the green exact-source `CI / verify` after both fixes, plus unaffected Linux/Windows workflows as applicable. These were operational dependencies of final Harness Lab branch qualification, not hard dependencies for writing Harness Lab code.

## 5. Invariants

Never weaken Clippy/warnings, skip/quarantine an integration test as if closed, hide SQLite busy errors, change corrupted-journal refusal semantics, increase arbitrary timeouts as sole fix, or make a broken test pass by not executing it. Reproduction and negative controls must distinguish product bug from fixture/teardown defect. Preserve isolation of CI test roots and the authoritative Tool Programs job lifecycle.

## 6. Closure and registration

The E001/E002 closure records cite exact implementation SHA, baseline failing CI, tests/guards actually run, compatibility and live contention semantics. Both closures and this roadmap/registry update are now recorded together. No paid provider was run, and no historical delegated-run or Harness Lab closure was rewritten.

## Closure verification (2026-10-26)

E001 and E002 are closed at source head `56279dfc7dc8ee82f0239d6412b4fa50a42a74d6`. Exact-head run [40383901325](https://github.com/dbowm91/codegg/actions/runs/40383901325) passed formatting, strict all-target Clippy, guards, Harness Lab Python tests, and the full workspace test sweep. The original Clippy and M015 lock incidents remain historical baseline evidence, not current blockers.
