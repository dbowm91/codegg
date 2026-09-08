# Development Verification and Release Milestone 010 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/development-verification-release/010-hosted-test-reproducibility-and-main-gate.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md#4-corrective-milestone`

Repository baseline reviewed: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Implementation commits:

- `d57580a0` — fix(verify): isolate child-shell test root for hosted reproducibility (DVR M010)
- `8fc7b3b5` — fix(scheduler): re-validate idempotency index under creation lock (DVR M010)

Accepted executable revision: `8fc7b3b59152c2e93a1f488cd03e409ed4d6f46b`

## 1. Executive finding

DVR M010 is complete. Two distinct hosted-only workspace-test failures were
diagnosed to root cause, fixed at their owning boundaries, and proven green
by one existing `CI / verify` run on the exact implementation SHA. No CI
lane, retry mechanism, release workflow, or verification framework was added,
and no test was ignored, skipped, or weakened.

1. `tool::bash::tests::isolated_child_shell_rejects_parent_paths_and_directory_changes`
   (run `34161421807`): test-fixture ownership defect. Fixed in the test only;
   the BUG-005 fail-closed production behavior is preserved.
2. `concurrent_identical_submissions_return_one_job_id`
   (run `34263458304`, exposed once defect 1 was fixed): production
   check-then-act race in `JobSubmissionService::submit`. Fixed in production
   code with an index re-validation under the creation lock.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Identify the exact test/process failure in run `34161421807` | Hosted log: `tool::bash::tests::isolated_child_shell_rejects_parent_paths_and_directory_changes ... FAILED`, panic at `src/tool/bash.rs:2225`, `assertion failed: validate_child_workspace_command("echo ok > /tmp/parent.txt", &[], &root).is_err()` | pass |
| Narrow reproduction command | `cargo test -p codegg --lib tool::bash::tests::isolated_child_shell --locked -- --test-threads=1`; defect class additionally demonstrated on macOS with a canonical-prefix-aligned root (`/private/tmp`), which fails the old fixture shape exactly as Linux `/tmp` does | pass |
| Root-cause classification for defect 1 | Test used shared `std::env::temp_dir()` itself as the isolated root. On Ubuntu `/tmp` is a real directory, so `/tmp/parent.txt` is inside the root and validation correctly allows it. On macOS `/tmp` symlinks to `/private/tmp`, so the lexical prefix check mismatched and the test passed locally. Introduced by `15632a04`, which changed the root from a (nonexistent) subdirectory to `temp_dir()` itself to satisfy the new fail-closed canonicalization | pass |
| Root-cause classification for defect 2 | Production race: concurrent `submit()` calls with the same `SubmissionKey` all miss the unlocked index/scan checks, then serialize on the creation mutex without re-checking, so each waiter creates its own durable job. Reproduced locally 2/15 runs before the fix | pass |
| Correct the root causes without weakening contracts | `d57580a0` (test fixture: owned `tempfile::tempdir()` root, outside path derived as sibling of the canonical root, plus inside-worktree allowed assertion); `8fc7b3b5` (production: re-validate the idempotency index under the creation lock, joining the existing job or surfacing a fingerprint conflict; stale entries fall through to creation) | pass |
| Focused regression fails before / passes after | Defect 1: old fixture shape fails under prefix-aligned root; fixed test passes. Defect 2: `concurrent_identical` failed 2/15 pre-fix loops, passes 30/30 post-fix loops; full `scheduler_submission_idempotency` binary 11/11; `scheduler::` lib suite 71/71 | pass |
| Exact hosted workspace-test command exits 0 | `cargo test --workspace --locked -- --test-threads=1` → `CARGO_EXIT=0`, 197 suites `ok`, lib `4309 passed; 0 failed` (log `/tmp/m010-workspace-tests-2.log`) | pass |
| Formatting / Clippy / guards not weakened | `cargo fmt --all -- --check` pass; `cargo clippy --workspace --all-targets --locked -- -D warnings` pass; `scripts/verify.sh quick` pass (agent schema, core boundary, sandbox, execution ownership, workspace check) | pass |
| Green existing `CI / verify` on the exact candidate SHA | Run `34267326594`, job `102199868254`, `head_sha` `8fc7b3b5`, conclusion `success`; both previously failing tests `ok` in that run | pass |
| No ignored tests, retries, or `continue-on-error` | `git diff` limited to `src/tool/bash.rs` test, `src/scheduler/submission.rs` production, and planning docs; `.github/workflows/ci.yml` untouched | pass |
| Branch-gate disposition | `main` is unprotected and no rulesets exist (API read); recorded as operator action below, no workflow workaround introduced | pass |

## 3. Production implementation evidence

`src/scheduler/submission.rs` (`8fc7b3b5`, +15 lines): after acquiring the
keyed-creation mutex, the in-memory idempotency index is re-validated before
`create_job_with_labels`. A waiter that lost the race now returns the
winner's durable job (fingerprint match), reports `SubmissionKeyConflict`
(fingerprint mismatch), or removes a stale entry and proceeds to creation
(job record gone). The pre-lock fast path, the unlocked durable restart
scan, the enqueue-failure cancel path, and lock ordering (idempotency mutex
outermost; store calls already occurred under it) are unchanged.

`src/tool/bash.rs` (`d57580a0`, test only): the isolated-child test uses an
owned `tempfile::tempdir()` root instead of the shared temporary directory,
derives the outside redirect target as a sibling of the canonical root, and
adds an inside-worktree redirect assertion. Production
`validate_child_workspace_command` semantics, including fail-closed
canonicalization, are unchanged.

No storage migration, protocol change, UI change, or static-guard change.
`architecture/testing.md`, `CONTRIBUTING.md`, and `AGENTS.md` needed no
update: every documented command and precondition was proven correct.

## 4. Verification executed

### Commands run

```bash
# narrow reproduction (defect 1)
cargo test -p codegg --lib tool::bash::tests::isolated_child_shell --locked -- --test-threads=1
# defect-class demonstration: old fixture shape under a prefix-aligned root -> FAILED, then restored -> ok
# narrow reproduction (defect 2, 15x loop pre-fix: 13 pass / 2 fail; 30x loop post-fix: 30 pass)
cargo test -q --locked --test scheduler_submission_idempotency concurrent_identical
cargo test --locked --test scheduler_submission_idempotency
cargo test --locked -p codegg --lib scheduler::

# exact hosted workspace-test contract (local, macOS)
cargo test --workspace --locked -- --test-threads=1   # CARGO_EXIT=0

# repository posture
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

### Results

Local (all on the final candidate unless noted):

- Isolated-child narrow tests: 2 passed (pre-fix defect-class demo: FAILED as
  designed; post-fix: ok).
- `scheduler_submission_idempotency` binary: 11 passed, 0 failed.
- `scheduler::` lib suite: 71 passed, 0 failed.
- Full workspace command: exit 0; 197 `test result: ok` lines, 0 failures;
  lib suite `4309 passed; 0 failed`. (Hosted Ubuntu lib total is 4314 due to
  platform-gated tests; the delta is platform gating, not skipped scope.)
- Formatting, workspace Clippy, and `scripts/verify.sh quick`: all passed.

Hosted exact-head evidence:

- Run `34267326594` —
  [GitHub Actions run](https://github.com/dbowm91/codegg/actions/runs/34267326594).
- Verify job `102199868254` —
  [hosted verify job](https://github.com/dbowm91/codegg/actions/runs/34267326594/job/102199868254).
- `head_sha` is exactly `8fc7b3b59152c2e93a1f488cd03e409ed4d6f46b`;
  conclusion `success`, including all static guards, formatting, workspace
  Clippy, and workspace tests.

Historical trigger evidence (unchanged, retained):

- Run `34161421807` / job `101863878469` at baseline `15632a04`: lib suite
  `4313 passed; 1 failed` on
  `isolated_child_shell_rejects_parent_paths_and_directory_changes`.
- Intermediate run `34263458304` / job `102186871494` at `d57580a0`: lib suite
  `4314 passed; 0 failed` (defect 1 proven fixed on hosted), then
  `scheduler_submission_idempotency` failed on
  `concurrent_identical_submissions_return_one_job_id`.
- Preceding-mainline hypothesis resolved: run `34154448225` at `f05fbfcd`
  failed on the unrelated `security_context_returns_risk_markers_for_source_file`
  (BUG-002, fixed by `15632a04`), not on either M010 defect.

## 5. Invariant review

- Routine hosted CI remains one bounded Ubuntu `verify` job; `.github/workflows/ci.yml` untouched.
- The workspace test step remains a hard failure gate; both failures were fixed, never masked.
- No `continue-on-error`, ignore, `#[ignore]`, retry loop, or rerun-until-green was added. The M009-style rerun was not used; each candidate has a first-attempt green run (`34267326594` succeeded on its only attempt).
- Production correctness failure (idempotency race) fixed in production code.
- Test-isolation failure (shared temp root) fixed at the fixture boundary with an owned subdirectory; no broad serialization added.
- Platform behavior is explicit: the outside path derives from the canonical root's parent, documented in-test for the macOS-symlink vs Linux-real cases.
- Core-boundary, sandbox, and execution-ownership guards still run and pass locally and on hosted.
- Hosted CI publishes nothing and acquires no release credentials.
- No new CI lane or verification framework.

## 6. Failure and recovery review

- Defect 1: no runtime failure semantics involved; fixture owned a shared path. Fixed by owning an isolated subdirectory (`tempfile::tempdir()`, auto-cleanup).
- Defect 2: concurrent same-key submissions previously created duplicate durable jobs. Now exactly one durable record is created per key per daemon lifetime; losers join the winner's job. Enqueue-failure cancellation and fingerprint-conflict semantics preserved. No new contention: the creation mutex was already held across create/enqueue; only a map lookup plus a `get_job` read were added under it, preserving lock order.
- Panic/unwind paths unchanged; no leaked tasks, sockets, files, or env mutation introduced.

## 7. Migration and compatibility review

No schema, protocol, configuration, or public-surface change. Test-fixture change has no migration requirement. The submission-service change is internal to daemon-owned admission; caller-visible behavior now matches the already-documented "creates exactly one durable record" contract.

## 8. Security review

No fail-open change. The bash workspace validator keeps fail-closed canonicalization; the test now exercises it with a root that exists on all platforms. The submission fix strictly reduces duplicate-job creation; conflict reporting for reused keys with different fingerprints is extended to the raced path. Sandbox, canonicalization, permission, credential, and ownership tests all pass.

## 9. Documentation and operations

No architecture-doc change was warranted (all documented commands verified correct). Branch-gate evidence: `GET repos/dbowm91/codegg/branches/main/protection` returns 404 "Branch not protected"; `GET .../rulesets` returns `[]`.

Operator action required: configure `main` to require the existing `CI / verify` status check (repository administration was not exercised by this milestone; no workflow workaround was introduced as a substitute).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | `validate_child_workspace_command` compares absolute tokens lexically against the canonical root, so an absolute token containing an interior `..` that still has the root as a lexical prefix (e.g. `<root>/../outside`) is allowed although it escapes the worktree. Pre-existing, not the demonstrated failure, and only the bare `..` token is rejected. | Bounded fail-open in a defense-in-depth validator. | Future scheduler/tool hardening milestone may normalize `..`/`.` segments or canonicalize token parents; explicitly out of M010 scope, not absorbed here. |
| low | `main` has no required `CI / verify` status check. | A future failing revision could merge without the gate M010 just proved. | Operator action above; no code work. |

No critical, high, or medium findings.

## 11. Roadmap disposition

M010 is strictly closed. The corrective addendum's deliverable boundary
(items 1-8) is fully satisfied, including the green exact-head run and the
branch-gate disposition. No corrective pass is required. The
development-verification-release workstream returns to closed; any future
verification defect receives a new milestone rather than reopening this one.

## 12. Registry updates

- `plans/registry.md`: Development verification and release subsystem
  `active` → `closed`, current milestone `M010 active` → `M010 closed`;
  remove the M010 row from dependency-ready plans; record this closure under
  recently completed control points.
- `plans/subsystems/development-verification-release-ci-reproducibility-corrective-addendum.md`:
  status `active` → `closed`.
- Dependency audit: no registered `blocked` plan lists DVR M010 as a hard or
  interface dependency (blocked work is runtime-safety C002 evidence,
  maintainability M002/M005, and distribution M002, all owned elsewhere), so
  no downstream plan is promoted by this closure. The independent
  ready handoffs (maintainability M001/M003/M004, provider-auth M010,
  distribution M001) are unaffected and remain `ready`.
