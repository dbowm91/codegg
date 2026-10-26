# Exact-HEAD CI Signal — October 26 Corrective Addendum

Status: active planning; E001 and E002 independently ready; **current main CI is not green** as of evidence below.
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
| E001 — all-targets Clippy exact-head restoration `plans/implementation/ci-exact-head-signal/001-clippy-gate-exact-head-restore.md` | ready | none |
| E002 — Tool Programs M015 crash/restart SQLite lock determinism `plans/implementation/ci-exact-head-signal/002-m015-restart-database-lock-determinism.md` | ready | none; reverify in hosted sweep after E001 |

E001 and E002 can be implemented separately; **final combined verification** is a green exact-source `CI / verify` after both fixes, plus unaffected Linux/Windows workflows as applicable. These are operational dependencies of final Harness Lab mainline qualification, not hard dependencies for writing Harness Lab code.

## 5. Invariants

Never weaken Clippy/warnings, skip/quarantine an integration test as if closed, hide SQLite busy errors, change corrupted-journal refusal semantics, increase arbitrary timeouts as sole fix, or make a broken test pass by not executing it. Reproduction and negative controls must distinguish product bug from fixture/teardown defect. Preserve isolation of CI test roots and the authoritative Tool Programs job lifecycle.

## 6. Closure and registration

Each E001/E002 closure in `plans/closure/ci-exact-head-signal/` must cite exact implementation SHA, baseline failing CI, failing-first proof, tests/guards actually run, old/new exit, compatibility and live contention semantics; update this roadmap and registry after acceptance. If E001 is merged while E002 remains flaky, status **conditional**, not green. Nothing here authorizes re-running paid providers or changes any Harness Lab M001–M005 historical closure.
