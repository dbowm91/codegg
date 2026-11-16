# Sandbox Containment Audit Corrective Milestone C004 — Containment Evidence Integrity

Status: conditionally closed

Repository baseline: `7642b9e381d9c91d650fa375be2a7aa0aae9bcd8`

Source roadmap:

- `plans/subsystems/sandbox-containment-audit-corrective-roadmap.md#milestone-c004--containment-evidence-integrity`

Long-term requirements:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#27-security-requirements`

Applicable ADRs:

- `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`

Primary class: invariant

## 1. Objective

Make the containment suite fail rather than skip when a qualified host is
expected, prove its denials are not vacuous on both platforms, and close the three
specific fidelity gaps that let a broken profile read as green.

## 2. Why this milestone is ready

No hard dependency. Every item is inside `tests/security_permission/` and the
`seatbelt.rs` unit module. The real-helper harness already exists
(`run_helper`, `spec_for`, `probe`, `require_backend!`), the macOS arm already
pins `BackendId::SEATBELT`, and the repository already contains the pattern this
milestone generalises: `linux_native_scratch_containment.inc` has the suite's only
defect-injection non-vacuity control.

The strict-mode marker defined here is consumed by **C005**, which sets it in the
macOS qualification lane. That is an interface this milestone provides, not a
dependency it waits on.

## 3. Current implementation evidence

**Every containment test can pass green without testing anything.**
`require_backend!` expands to an early `return` after an `eprintln!` when
`platform_sandbox_capability()` is `Unavailable`. All 21 macOS-reachable
containment tests use it. A host where `sandbox-exec` was removed, or where
Landlock is not enabled, reports green — which is the exact failure the macOS
lane's `/usr/bin/sandbox-exec` presence step exists to catch, and that step is a
separate shell command rather than an assertion the test makes.

**The rename/link escape test does not run on macOS.**
`landlock_blocks_rename_and_link_from_workspace_into_outside_roots` is gated
`cfg(target_os = "linux")` although its body is platform-neutral — it drives
`probe`, which works on both. The audit verified empirically on macOS 26.6.2 that
the generated profile **does** block hardlink creation, APFS `clonefile`, and
cross-root `rename`; nothing in the suite asserts it, so a future narrowing of
`(deny file-write*)` would go unnoticed on the platform where that rule is the
only write boundary.

**No macOS non-vacuity control exists.**
The Linux scratch fixture deliberately widens a grant and observes the difference,
which is what keeps its nine `=denied` observations from being vacuous. The
containment matrix has no equivalent on either platform.

**One unit test asserts a branch production never takes.**
`seatbelt::tests::read_only_profile_denies_every_write` builds
`BackendPolicy::new(read, write = vec![], deny)` and therefore exercises the
`write_roots.is_empty()` arm. Production `ReadOnly` always pushes
`device_write_allowances()` into `write_paths`, so the real profile takes the
else arm and **does** contain `(allow file-write* …)`. The integration test
`read_only_denies_even_the_workspace` covers the production shape, so this is a
fidelity defect in the unit test rather than a coverage hole — but it asserts a
property (`no write re-allow at all`) that is false of the shipped profile.

**One assertion lacks a positive control on its healthy path.**
`denied_credential_nested_under_workspace_is_not_read_or_written`'s write half
asserts the credential content is unchanged, and consults its `target-ran` marker
only in the `backend().is_none()` branch. When a backend did enforce, nothing
proves the child executed, so "the write was denied" and "the child never ran" are
indistinguishable. (The read half is sound: `probe` requires a decodable
`Enforced` frame, and `Enforced` is written before `exec`.)

## 4. Invariants that must not regress

- A containment test never reports success for a run that did not execute a
  contained child.
- Denial observations are non-vacuous: an injected widening is observable.
- The suite still passes on a host that genuinely has no backend, unless strict
  mode is requested.
- No test reads or writes real user credential material.
- No production code changes in this milestone.

## 5. Scope

### In scope

- `require_backend!` and the strict-mode marker contract.
- The rename/link test's platform gate.
- A defect-injection non-vacuity control for the containment matrix.
- `seatbelt::tests::read_only_profile_denies_every_write` fidelity.
- The `denied_credential_nested_under_workspace` positive control.

### Explicitly out of scope

- Setting the marker in any workflow (C005).
- Adding the curated-list kernel fixture (C001).
- Any production change.
- Any new test family or CI lane.
- Windows.

## 6. Required production changes

None. This milestone changes tests and their harness only. That is an acceptance
criterion, not a limitation: `src/**` must be untouched.

### Documentation and static guards

The strict-mode marker is documented where a maintainer will look for it — the
containment fixture's module docs and `architecture/testing.md`'s family table if
it describes the member — so that a red lane is diagnosable.

## 7. Ordered work packages

### Work package A — Strict-mode marker

Intent: a qualification lane must not be able to go green from a skip.

Required changes: define a marker read by `require_backend!`. When the marker is
set and the capability is `Unavailable`, the test **fails** with the platform
reason instead of returning. When it is unset, behaviour is unchanged so a
developer on a host without a backend still gets a green run.

Constraints: the marker name must not be `CODEGG_SANDBOX_HELPER` — that name is
reserved for helper identity and `scripts/check_sandbox_contract.py` rejects its
use for that purpose. Use a distinct name and document it. Do not read the marker
in production code; it belongs to the test harness only, and the guard's
production scan must stay clean.

Acceptance evidence: with the marker set on a host without a backend, the member
fails and names the reason; with it unset, the same host is green. Both observed.

### Work package B — Run the rename/link escape test on macOS

Intent: the write boundary is the only write protection macOS has, and its escape
classes must be asserted there.

Required changes: remove the Linux-only gate if the body is genuinely
platform-neutral, or split it into a shared body plus per-platform assertions.
Keep the existing Linux assertions unchanged. Add the macOS arm asserting
`RENAME-OUTSIDE-DENIED` and `LINK-OUTSIDE-DENIED` with the same
`!target.exists()` checks.

Acceptance evidence: the member runs on macOS and fails if `(deny file-write*)` is
narrowed — demonstrate by narrowing it locally and observing the failure.

### Work package C — A non-vacuity control for the containment matrix

Intent: prove the denial probes can observe an allowed case.

Required changes: add a fixture that constructs a policy whose grant is
deliberately widened (for example an extra write root or a removed deny), runs the
same probe shape as a neighbouring denial test, and asserts the **opposite**
outcome. Model it on `linux_native_scratch_containment.inc`'s control. It must
work on macOS and on Linux.

Acceptance evidence: the control fails if the widening is removed, i.e. it cannot
pass by accident.

### Work package D — Correct the read-only unit test

Intent: a unit test must assert a property of the shipped profile.

Required changes: build the policy the way production does — include the device
write roots — and assert what is actually true: every write outside the device
nodes is denied, and no workspace write root is re-allowed. Keep or rename the
test so its name states the property it now checks.

Acceptance evidence: the test fails if a workspace write root is re-allowed.

### Work package E — Positive control for the nested-deny write half

Intent: "denied" and "never ran" must be distinguishable.

Required changes: make the `target-ran` marker assertion unconditional — the
child must have executed for the credential to have been attempted. If the child
legitimately cannot run on some host, that host must be the strict-mode failure,
not a silent pass.

Acceptance evidence: the test fails if the child never executes.

## 8. Failure, cancellation, restart, and contention semantics

Test-only. The strict-mode marker is read once per test process; `ci` profile runs
one process per test, so no cross-test state. No restart or contention semantics.

## 9. Compatibility and migration

No production, configuration, or protocol change. A developer's local run without
the marker behaves exactly as today. The marker is not a durable preference and is
not read by the daemon.

## 10. Required tests

### Focused unit tests

- `seatbelt::tests` read-only fidelity (work package D).
- The marker's two modes, asserted by a test that sets and clears it in-process
  where the profile allows, or by a documented manual demonstration where it does
  not.

### Integration tests

- The rename/link member on macOS and Linux.
- The non-vacuity control on both platforms.
- The nested-deny write half with its positive control.

### Restart and recovery tests

Not applicable.

### Contention and cancellation tests

Not applicable.

### Security and negative tests

- Every added denial assertion is paired with an injected widening that must make
  it fail.

### Migration and compatibility tests

- A host without a backend stays green when the marker is unset.

## 11. Required verification commands

```bash
# containment family, both platforms where available
cargo test -p codegg --test security_permission -- sandbox_containment

# strict mode on a host that has a backend
CODEGG_SANDBOX_QUALIFICATION=required \
  cargo test -p codegg --test security_permission -- sandbox_containment

# seatbelt unit module
cargo test -p codegg --lib security::sandbox::seatbelt

# static guards that constrain this surface
python3 scripts/check_sandbox_contract.py
python3 scripts/check_sandbox_contract.py --self-test
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_test_common_binding.py

# formatting and lint
cargo fmt
cargo clippy -p codegg --all-targets --locked -- -D warnings
```

`check_test_assertion_quality.py` and `check_test_duplication.py` are wired into
`quick` and CI; a new test body that asserts nothing, or that duplicates an
existing assertion, fails them. Do not add a body to `docs/test-assertion-quality.toml`
to get green — record it only if the test genuinely asserts nothing and say why.

## 12. Documentation updates

- The containment fixture's module docs: the marker's name, meaning and intended
  caller.
- `architecture/testing.md` if its family table or sandbox guidance describes the
  member.
- The roadmap §12 status table and `plans/registry.md` on closure.

## 13. Acceptance criteria

1. With the strict-mode marker set, an unavailable backend **fails** the
   containment member and names the reason; with it unset, the same host is green.
2. The rename/link escape assertions run on macOS and fail when
   `(deny file-write*)` is narrowed.
3. A defect-injection control proves a widened grant is observable on both
   platforms.
4. The read-only unit test asserts a property of the shipped profile, including the
   device write roots.
5. The nested-deny write half proves the child executed.
6. `git diff --stat` shows no file under `src/**` changed except the `seatbelt.rs`
   unit-test module.

## 14. Stop conditions

Stop and report rather than improvise when:

- the rename/link body is not platform-neutral and splitting it would weaken the
  Linux assertions;
- the non-vacuity control cannot be expressed without production code;
- the strict-mode marker would have to be read by production code to work;
- a new test body cannot assert a checkable property and would need a
  `docs/test-assertion-quality.toml` entry — that is a design smell in the test,
  not a registry problem;
- fixing the read-only unit test reveals that production `ReadOnly` really does
  omit device writes, which would be a product defect and belongs to a new finding.

## 15. Closure evidence required

- The two marker modes observed, with the host state described.
- The macOS rename/link run, plus the narrowing demonstration that makes it fail.
- The non-vacuity control on both platforms, plus the demonstration that removing
  the widening makes it fail.
- The read-only unit test's new assertions and the property they check.
- The nested-deny positive control.
- Confirmation that no production file changed, with the `git diff --stat`.
- The roadmap §12 and registry updates, including the unblock audit.

## 16. Handoff notes

- The strict-mode marker is this milestone's **interface** to C005. Fix its name
  here, document it, and do not rename it later without updating the workflow.
- `require_backend!` is a macro; changing it changes every user at once. Check the
  unset path on a real host before landing.
- Do not weaken `probe()`'s `backend().is_none()` guard. The read half's soundness
  depends on it.
- This plan and **C001** both extend `sandbox_containment.inc`. C001 owns the
  curated-list fixture; this plan owns the harness and the escape/non-vacuity
  tests. Coordinate the shared helpers rather than duplicating them.
- `check_test_duplication.py` will reject a new body that asserts the same thing as
  an existing one. The non-vacuity control asserts the *opposite* outcome, which is
  not a duplicate — but say so in the test's own comment so the next reviewer does
  not have to infer it.
