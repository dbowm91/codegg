# Sandbox Containment Audit Corrective C004 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/sandbox-containment-audit-corrective/004-containment-evidence-integrity.md`

Source subsystem roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c004--containment-evidence-integrity`

Repository baseline reviewed: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8` (`feat-auto-20261109-f8abad0d` HEAD) on a macOS 26.6.2 host (Darwin 25.6.0, arm64, `sandbox-exec` present, backend `Seatbelt`).

Implementation commits or pull requests:

- **None. The implementation is uncommitted working-tree work**, shared with sibling agents
  (C001, C002, C005, C006) on `feat-auto-20261109-f8abad0d`. No commit, stage, or push
  identifies this milestone.

## 1. Executive finding

The containment suite can no longer go green from a skip on a qualification lane, and its
denials are no longer unfalsifiable on macOS. `require_backend!` now reads
`CODEGG_SANDBOX_QUALIFICATION` (`tests/security_permission/sandbox_containment.inc:289-323`):
unset or `optional` keeps the skip, and **any other value — including the empty string —
fails the member with the platform's own reason**. The rename/link escape test lost its
Linux-only gate and runs here on Seatbelt, asserting `RENAME-OUTSIDE-DENIED`,
`LINK-OUTSIDE-DENIED` and `!target.exists()`. A defect-injection control
(`a_widened_write_grant_is_observable_by_the_same_probe`, `:828-860`) injects a write root
the denial fixture does not grant and asserts the **opposite** outcome from the same probe
shape — it passed here, which is what makes the other denials non-vacuous on this platform.
The `read_only` unit test was corrected to assert the shipped profile's shape rather than a
branch production never takes, and the nested-deny write half now asserts unconditionally
that the contained child ran.

The whole suite passed 22/22 with the marker unset and 22/22 with it set to `required`, so
strict mode does not change a capable host's result.

Two evidence items are outstanding and are named in §10 and §11 rather than papered over:
the Linux half of the non-vacuity control — which runs in `ci.yml`'s `ubuntu-latest` job
(`:659-663` builds the helper, `:665-680` runs the sweep), so it is one committed-and-pushed
run away rather than host-blocked — and the "narrow the grant and watch it fail"
demonstrations, which are mutation tests over production or fixture code and need a private
tree, not a host.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 With the marker set, an unavailable backend fails and names the reason; unset, the same host is green | `qualification_required` is `marker.is_some_and(|v| v != "optional")`; `the_qualification_marker_turns_an_absent_backend_into_a_failure` asserts unset/`optional`/`required`/`""` and passed | Partial | The four branch values are observed. The end-to-end "host without a backend" state is not producible here: `platform_sandbox_capability()` probes the real backend and there is no override |
| §13.2 The rename/link escape assertions run on macOS and fail when `(deny file-write*)` is narrowed | `rename_and_link_from_the_workspace_into_outside_roots_are_denied` (`:712-748`) has no platform gate and passed in the 22/22 macOS run | Pass (run) / not demonstrated (fails-when-narrowed) | The narrowing demonstration is a mutation over `seatbelt::sbpl_profile`; see §10 |
| §13.3 A defect-injection control proves a widened grant is observable | `a_widened_write_grant_is_observable_by_the_same_probe` passed on macOS | Partial — Linux half not run | The control asserts `WIDENED-LEAK` and `outside_new.exists()`, the opposite of `WRITE-OUTSIDE-DENIED`. The Linux half runs in `ci.yml`'s `ubuntu-latest` job (`:665-680`); the blocker is the commit, and that job does not set the qualification marker, so the member would skip there rather than run |
| §13.4 The read-only unit test asserts a property of the shipped profile, including device write roots | `read_only_profile_re_allows_only_the_device_nodes` (`src/security/sandbox/seatbelt.rs`) passed in the seatbelt module (8/8) | Pass | Compares the re-allow count and contents against `policy::device_write_allowances()` and asserts no workspace root is re-allowed |
| §13.5 The nested-deny write half proves the child executed | `:365-372` asserts `marker.exists()` unconditionally with the outcome and stderr in the failure message | Pass (run) | Previously consulted only in the `backend().is_none()` branch |
| §13.6 `git diff --stat` shows no file under `src/**` changed except the `seatbelt.rs` unit-test module | `git diff -- src/security/sandbox/seatbelt.rs` is the 40-line test-module change only | Pass for this milestone's own contribution | The tree-wide stat shows 36 src files because C001/C002/C006 and unrelated TUI work share the uncommitted tree; see §4 |

## 3. Production implementation evidence

**No production behaviour changed, by design.** The plan's acceptance criterion is that
`src/**` is untouched except the `seatbelt.rs` unit-test module, and that is what this
milestone contributes: a test function renamed from `read_only_profile_denies_every_write`
to `read_only_profile_re_allows_only_the_device_nodes`, rebuilt on the policy production
actually constructs (`BackendPolicy::new(read, device_write_allowances(), deny)`) and
asserting the positive and negative facts that follow — every re-allow is a device node,
every device node is re-allowed, and no workspace write root is re-allowed.

**Harness.** `tests/security_permission/sandbox_containment.inc` gained:
`QUALIFICATION_MARKER`, `qualification_required`, the marker test, and the failed-backend
arm inside `require_backend!`; the rename/link member un-gated; the non-vacuity control; the
unconditional `target-ran` assertion; and the module-doc paragraph describing the marker's
contract (`:23-28`).

**Documentation.** `architecture/testing.md:122-129` documents the marker beside the skip
taxonomy: "unset, or set to `optional`, keeps the skip … any other value makes
`require_backend!` panic with the platform's own reason", naming the qualification lane as
the intended caller.

## 4. Verification executed

### Commands run

```bash
cargo build --locked --bin codegg-sandbox-helper
cargo test --locked --test security_permission -- sandbox_containment
cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
CODEGG_SANDBOX_QUALIFICATION=required \
  cargo test --locked --test security_permission -- sandbox_containment --test-threads=1
cargo test --locked --test security_permission -- \
  sandbox_containment::seatbelt_preserves_and_reports_preopened_denied_file_descriptors --nocapture
cargo test --locked -p codegg --lib security::sandbox::seatbelt
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_test_common_binding.py
cargo fmt --check --all
```

### Results

- Serial run: **22 passed, 0 failed.** Strict-mode run: **22 passed, 0 failed.** Both
  report `324 filtered out` (the rest of the `security_permission` family).
- The default parallel run was **21 passed, 1 failed**: `seatbelt_preserves_and_reports_
  preopened_denied_file_descriptors` failed with `target failed: sandbox helper status
  failure: write sandbox status: Bad file descriptor (os error 9)`. Running that member alone
  passed, and the serial run passed. The symptom is consistent with a descriptor-number
  collision in the fixture's `pre_exec` (`launch_helper_with_inherited_fd` duplicates into
  fixed fd 42 and the status pipe is dup2'd to fd 3, so a pipe write end already at fd 42
  would leave the wrong object at fd 3) — a diagnosis from the fixture's fixed numbers, not an
  instrumented observation. It is not a containment defect. Recorded in §10.
- `cargo test -p codegg --lib security::sandbox::seatbelt` — **8 passed**, including
  `read_only_profile_re_allows_only_the_device_nodes`.
- `check_sandbox_contract.py` and `--self-test` — passed.
- `check_test_assertion_quality.py` — passed (no entry was added to
  `docs/test-assertion-quality.toml` to obtain green).
- `check_test_duplication.py` — passed; the non-vacuity control's header comment states that
  it asserts the opposite outcome, so the duplication guard did not need an exception.
- `check_test_common_binding.py` — passed.
- `cargo fmt --check --all` — **failed at this review**, one file: `src/main.rs:5426`
  (`temp_measure_catalog_open`). Not a sandbox file and not this milestone's change; at the time
  it was why `scripts/verify.sh quick` aborted before its guard phase in this tree.
  **Discharged 2026-11-13** by `scripts/verify.sh full`: `cargo fmt --check` and clippy are clean
  and the sweep passed (17,377 workspace + 8,441 feature-pass tests, 0 failures), so `quick` no
  longer aborts before its guards.
- **`git diff --stat -- src/`** shows 36 files / 5815 insertions / 573 deletions. That figure
  cannot be attributed to this milestone: the same uncommitted tree carries C001
  (`policy.rs`), C002 (`landlock.rs`, `backend.rs`), C006 (`managed_process.rs`,
  `linux_mount_view.rs`, `bin/codegg-sandbox-helper.rs`) and unrelated TUI/config/protocol
  work. **C004's own `src/` contribution is the `seatbelt.rs` test module and nothing else.**

## 5. Invariant review

- **A containment test never reports success for a run that did not execute a contained
  child.** The dead-skip is now a failure under the marker, `probe()` still requires a
  decodable `Enforced` frame, and the nested-deny write half asserts `target-ran`
  unconditionally.
- **Denial observations are non-vacuous.** The injected-widening control asserts the allowed
  case with the same probe shape and passed on macOS; the Linux counterpart was not run.
- **The suite still passes on a host that genuinely has no backend unless strict mode is
  requested.** The branch logic is asserted in-process for all four marker values. The host
  state itself is not reproducible here.
- **No test reads or writes real user credential material.** The curated fixture relocates
  every path under a temporary root; the pre-opened-descriptor fixture writes its own
  sentinel under a temporary `$HOME`-rooted directory.
- **No production code changes.** `seatbelt.rs`'s test module only.

## 6. Failure and recovery review

Test-only; no durable state, no restart, no cancellation path. The marker is read once per
test process, and the `ci` profile runs one process per test, so there is no cross-test
state. The one reliability defect found is the parallel descriptor race described in §4 and
§10.

## 7. Migration and compatibility review

No production, configuration, or protocol change. A developer's local run with the marker
unset behaves exactly as before, which the 22/22 unset run confirms. The marker is not a
durable preference and is not read by production code: the production scan in
`check_sandbox_contract.py` (which rejects `CODEGG_SANDBOX_HELPER` misuse and stderr marker
parsing) passes, and the marker lives only in the fixture.

## 8. Security review

- The non-vacuity control is the security-relevant deliverable: without it, a containment
  denial that stopped being observed would keep reporting pass.
- The marker removes the specific failure mode the macOS lane exists to catch — a release
  that removes `/usr/bin/sandbox-exec` previously produced a green skip.
- No secret is introduced: the fixtures use synthetic sentinel content under temporary roots.
- The read-only unit test now asserts the shipped profile, so a future re-allow of a
  workspace write root under `ReadOnly` fails a test instead of passing one.

## 9. Documentation and operations

- `tests/security_permission/sandbox_containment.inc:23-28` — the marker's name, meaning,
  two modes, and intended caller.
- `architecture/testing.md:122-129` — the marker beside the skip taxonomy, so a red
  qualification lane is diagnosable.
- `docs/test-ownership.toml` needs no change: the existing `security_permission` group
  already selects `src/security/**`.
- No new guard. `check_sandbox_contract.py --self-test`, `check_test_assertion_quality.py`
  and `check_test_duplication.py` constrain this surface and were run.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Medium | The non-vacuity control has not been executed on Linux | Plan §13.3 says "both platforms"; the Linux half is unobserved, so a Linux-only break in the control would not be caught by this record | In `ci.yml`'s `ubuntu-latest` job, after WP 0 of `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md` commits the tree, with `CODEGG_SANDBOX_QUALIFICATION=required` set: `cargo test --locked --test security_permission -- sandbox_containment a_widened_write_grant_is_observable_by_the_same_probe --test-threads=1`. Without the marker that job would skip rather than run (`002-status.md` §10) |
| Medium | The "narrow the grant and watch it fail" demonstrations were not re-executed | §15 of the plan asks for each: `(deny file-write*)` narrowed (AC2), the widening removed (AC3), a workspace write root re-allowed (AC4). Each requires mutating `seatbelt.rs`'s profile builder or the fixture, outside this record's write scope while sibling agents hold the same tree | On a private tree: make each single-line mutation, observe the named member fail, revert, and record the failure text |
| Low | The unavailable-backend state cannot be produced on this host | AC1's end-to-end arm (marker set on a host with no backend → member fails with the reason) is unobserved; only the branch logic is | Run on a host without `/usr/bin/sandbox-exec` (a container or a macOS image with it removed) with and without `CODEGG_SANDBOX_QUALIFICATION=required` |
| Low | `seatbelt_preserves_and_reports_preopened_denied_file_descriptors` is unreliable under the default parallel harness | A local parallel run can report a spurious containment failure; the lane and this record use `--test-threads=1` | Make the inherited descriptor number dynamic, or mark the member serial-only, so a developer's default run is trustworthy |

No high-severity finding. The remaining items are evidence gaps about *demonstrations*, not
missing implementation: every fixture the plan requires exists, and every one that can run on
this platform ran and passed.

## 11. Roadmap disposition

**Milestone conditionally closed.** Production (test-side) implementation is complete and the
macOS evidence is observed; the outstanding items are named external/operational evidence and
two mutation demonstrations that this record's write scope excludes. Conditions:

1. **Linux control run** — the command in §10, run in `ci.yml`'s `ubuntu-latest` job with
   `CODEGG_SANDBOX_QUALIFICATION=required` set and recorded with its output. The blocker is the
   commit (WP 0 of
   `plans/implementation/five-line-closure-corrective/001-closure-evidence-and-residue.md`), not
   a host: that job already builds the helper (`:659-663`) and runs the family (`:665-680`).
2. **Mutation demonstrations** — the three single-line mutations in §10, each observed to fail
   the member it protects, then reverted. These need a private tree, not a host.
3. **Unavailable-backend arm** — the marker on a host with no backend, both modes observed. No
   CI lane is configured to produce that state, so this stays a manual observation on a host with
   the backend removed.

The roadmap exit condition is written for macOS ("a defect-injection control proves a widened
grant is observable on macOS as it already is on Linux"), and that half is **satisfied**; it is
the plan's stricter "both platforms" wording that keeps this record conditional, and the Linux
half of that wording is one committed-and-pushed run away.

## 12. Registry updates

- Roadmap §12 status table: C004 → `conditionally closed` with the three conditions and the
  closure record `plans/closure/sandbox-containment-audit-corrective/004-status.md`.
- `plans/registry.md`: the subsystem row's "no work started" state is inaccurate; C004's row
  moves from `ready` to `conditionally closed`.
- Unblock audit: C005's interface dependency on C004's strict-mode marker is **satisfied** —
  the marker exists, is documented, and is set by the qualification lane
  (`.github/workflows/shared-checkout-macos-qualification.yml`).

## Correction log (2026-11-13)

Status unchanged: **`conditionally closed`**. The gate-class correction names its discharge; the
rest name the superseding document or the measured tree state.

- **§1, "the Linux half of the non-vacuity control" as an outstanding item.** Corrected: it is
  `ci.yml`'s `ubuntu-latest` job (`:659-663`, `:665-680`), one committed run away; the
  demonstrations need a private tree, not a host.
- **§2, §13.3 note "Partial — Linux half not run."** Corrected to name the lane, the commit as
  the blocker, and the marker gap that would make the member skip.
- **§4, the `cargo fmt --check --all` failure and "it is why `scripts/verify.sh quick` aborts
  before its guard phase".** Corrected in place: **`scripts/verify.sh full` on 2026-11-13** is
  green with fmt and clippy clean and 17,377 workspace + 8,441 feature-pass tests at 0 failures,
  so `quick` no longer aborts. The failure was real at the review snapshot and was never this
  milestone's change.
- **§10, Medium non-vacuity required action, "On a Linux host:".** Corrected to the
  `ubuntu-latest` job after WP 0 (the commit), with the marker set so the member cannot skip.
- **§11 condition 1, "on a Linux host".** Corrected to the `ubuntu-latest` job with the marker
  set and the commit named as the blocker.
- **§11 condition 3, "the marker on a host with no backend".** Clarified, not weakened: no CI
  lane produces a backend-less host, so the observation stays manual. The condition itself is
  unchanged.
