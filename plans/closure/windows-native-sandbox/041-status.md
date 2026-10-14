# Windows Native Sandbox C041 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/windows-native-sandbox/041-mainline-windows-ci-and-test-only-build-boundary.md`
Source subsystem roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0027-capability-stratified-successor-handoff-post-pr-111-merge-2026-10-13`
Repository baseline reviewed: `edde2bebe`
Implementation commits: `edde2bebe` — feat(windows-sandbox): C041 mainline CI/build guard, C042 guarantee contract, C044 two-hop harness, C047 coverage audit

## 1. Executive finding

C041 is closed as a maintenance/packaging guard. The mainline launcher
workflow now triggers on `main` push and `pull_request` (not only the
legacy foundation branch), carries a concurrency group, keeps the
historical C014 negative reproduction opt-in manual, and documents that
costly MXC/eligibility research stays out of this gate. A deterministic
release guard (`scripts/check_windows_test_support_release.py`) proves
the `windows-sandbox-test-support` feature cannot ship in distributed
binaries. No backend is admitted, no schema or permission changes, no
host side effects. Windows production `SandboxRequest::Required`
remains unavailable/fail-closed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Mainline + PR launcher triggers with path filters | `.github/workflows/windows-process-launcher.yml` push branches `[main, work/windows-native-sandbox-foundation]` + `pull_request` paths | pass | Same production path set as before; research workflows untouched |
| Concurrency guard, no costly research on ordinary PRs | `concurrency: windows-process-launcher-${{ github.ref }}`, MXC/eligibility workflows remain manual/path-scoped | pass | Documented in workflow header comment |
| C014 stays opt-in manual | `run_c014_research` boolean retained, `c014-negative-reproduction` job unchanged | pass | Historical negative preserved |
| Release guard: feature cannot ship | `scripts/check_windows_test_support_release.py` (6 check groups + `--self-test`) + existing `src/lib.rs` release `compile_error!` | pass | Local: guard passed, self-test passed |
| Test-only code stays test-only | `Cargo.toml` empty feature, `required-features` on both `[[test]]` targets, `cfg`-gated `src/managed_process.rs` entry + fail-closed spec check | pass | Guard asserts each |
| No Unix CI regression | `check_sandbox_contract`, `check_adr_integrity`, `check_execution_ownership` passed; `cargo fmt --check`, `cargo check -p codegg --lib`, sandbox lib tests passed | pass | Windows native runs not-run on darwin (named limitation) |
| No cross-host security inference | Server launcher vs Windows 11 ARM MXC jobs stay separate; no inference added | pass | Code/docs review |

## 3. Production implementation evidence

- `.github/workflows/windows-process-launcher.yml`: added `main` to
  push branches, added full `pull_request` path filter block, added
  `concurrency` group with `cancel-in-progress`, added header comment
  separating this gate from MXC/eligibility research. No job steps,
  hosts, or test commands changed.
- `scripts/check_windows_test_support_release.py` (new): six
  deterministic checks (feature not default + empty, release
  `compile_error!`, test-target gating, release scripts clean, source
  `cfg` gating + fail-closed spec, workflow triggers) plus
  `--self-test`. No network, no build.
- No changes to `src/managed_process.rs` dispatch, `SandboxConfig`,
  profiles, approval, or release packaging scripts. `src/lib.rs`
  release gate unchanged and re-verified.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_windows_test_support_release.py
python3 scripts/check_windows_test_support_release.py --self-test
python3 scripts/check_sandbox_contract.py
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
cargo fmt --check --all
git diff --check
cargo check -p codegg --lib --locked
cargo test -p codegg --lib security::sandbox::backend -- --nocapture
cargo test -p codegg --lib security::sandbox::guarantee -- --nocapture
```

### Results

Local (darwin, branch `planning/windows-sandbox-capability-successors`
at `edde2bebe`): all guards passed; fmt clean; diff clean; `cargo
check` finished `dev` profile in 2m35s exit 0; backend 4/4 tests
passed; guarantee 10/10 tests passed. Hosted Windows runs (launcher,
probe-contract) were not run from this host — recorded as not-run, not
as pass. No hosted CI run is claimed as closure evidence.

## 5. Invariant review

- Explicit profiles/ceilings unchanged; no backend preference change.
- No untrusted target starts before containment: dispatch untouched,
  test-only path still requires explicit spec and fails closed.
- Filesystem/network/process facts still reported separately; network
  stays `Unrestricted` for all shell paths.
- Landlock/Seatbelt guarantees, Unix helper channel, approval
  semantics intact (guards + lib tests green).
- No original-host ACL/SACL/MIC mutation, no persistent account, no
  FullHost fallback.

## 6. Failure and recovery review

Workflow concurrency cancels superseded runs on the same ref; no new
partial-failure mode. Release guard fails closed (nonzero exit with
named violations). No daemon restart, lease, contention, or artifact
behavior changed — guard-only milestone.

## 7. Migration and compatibility review

No schema, migration, protocol, config, or rollback surface changed.
Old serialized profiles map as before. Release archive contract
(`scripts/release/`) untouched and re-verified clean of the feature.

## 8. Security review

No authorization, secret, path-validation, privilege, DoS, redaction,
or audit change. The guard reduces shipping risk (test-only code in a
release binary is now mechanically rejected); it grants no new
authority.

## 9. Documentation and operations

- Workflow header comment documents the gate's scope vs research.
- Guard script usage documents all six checks.
- Roadmap §ADR-0027 handoff and registry C041 row to be updated to
  closed in the closure commit (see §12).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted Windows launcher run at this SHA not executed | No exact-SHA native proof for the trigger change itself | Next Windows-affecting push/PR exercises the gate; record run id then |
| low | Release `--release + feature` negative build not executed | Guard is static, not a compiled negative proof | Optional future: CI job attempting the forbidden combination and asserting failure |

No medium/high/critical findings. Neither low finding blocks closure:
the milestone is CI/packaging guards verifiable on Linux, and both
gaps are named future evidence, not missing acceptance proof.

## 11. Roadmap disposition

Milestone closed; C042 (already closed in the same implementation
commit) was the only interface-adjacent front and needed no C041 gate.
C043's hard C042 dependency is satisfied by the C042 closure; C043
remains blocked on its operational eligible-Windows-11 host gate only
(see C043 closure). No other plan is unblocked by C041 alone.

## 12. Registry updates

- `plans/registry.md`: C041 row `ready` → `closed` (implementation
  `edde2bebe`, closure `plans/closure/windows-native-sandbox/041-status.md`).
- `plans/subsystems/windows-native-sandbox-roadmap.md` milestone table:
  C041 `ready` → `closed`; C042 → `closed`; C044 → `blocked
  (harness landed; Windows execution not-run)`; C043 stays `blocked`
  with updated blocker (C042 satisfied, host gate remains); C047 →
  `closed (audit slice; production integration deferred)`; C045/C046/C048
  stay `blocked` with unchanged blockers.
- Implementation plan `041-*.md` status: `ready` → `implemented; closed`.
- Unblock audit: C041 unblocks nothing by itself; combined C041+C042
  closure enables the C047 audit slice (closed herewith) and satisfies
  C043's contract hard dependency (host gate still blocks execution).
