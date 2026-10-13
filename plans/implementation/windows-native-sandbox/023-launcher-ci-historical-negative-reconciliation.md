# Windows Native Sandbox C023 — Windows Launcher CI and Historical-Negative Reconciliation

Status: ready for handoff

Repository baseline: `4d61f9622ee037313ce4781eb5b5666142d95a5d` on `work/windows-native-sandbox-foundation`; refresh HEAD before implementation.

Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`.

Long-term requirements: `plans/000-long-term-specification.md` sections 4, 17, 27 and 29; `plans/001-terminology-and-domain-model.md` section 8.

Applicable ADRs: ADR-0004 (approval/profile), ADR-0016 (sandbox launch authority), ADR-0019 (historical AppContainer experiment), ADR-0020 (feasibility only).

Primary class: security invariant / infrastructure.

## 1. Objective

Make the mandatory Windows launcher CI a truthful, consistently executed regression gate. The historically negative C014 AppContainer/Git experiment must stop masking positive launcher tests. Keep its reproducer and findings; never turn a failing security probe into a false pass.

## 2. Readiness

C014 was closed negative at `plans/closure/windows-native-sandbox/014-status.md`. C019–C022 are completed or conditionally closed research. No architectural decision, elevation or production backend is required. C024 can proceed independently in parallel.

## 3. Evidence

At baseline, `.github/workflows/windows-process-launcher.yml` runs `windows_projection_feasibility` at step 9 as mandatory. Native run [38336513511](https://github.com/dbowm91/codegg/actions/runs/38336513511) failed that step; launcher, suspended-process, ACL, status and shell checks following it were **skipped**. Repeated runs 38336342483 and 38335783447 show the same pattern. C014's real Git `/dev/null` failure is already a closed negative finding, not a pending success gate. Workflow also duplicates later test steps. Independent C019 and C021 workflows passing does not replace skipped regression tests.

## 4. Invariants

- Production Windows `SandboxRequest::Required` remains unavailable; no `Enforced`, unrestricted fallback, sandbox admission or network guarantee.
- Preserve C014 test and known-negative closure; never use blanket `continue-on-error` to hide new failures.
- All previously qualified positive launcher/Job/ACL/environment/status/shell regressions must run with nonzero selected test counts.
- Maintain suspended-before-Job assignment and daemon `ManagedProcessService` ownership. No source workspace ACL mutations.

## 5. Scope

Change the Windows process-launcher workflow, exact test selection and testing documentation. Move C014 to a **clearly labeled explicit research/manual reproduction path** (or independent opt-in job); keep code and historical run reference. Remove duplicate test invocations without reducing distinct coverage. Do not rework AppContainer/Git, MXC, principal provisioning, security policy or other subsystems.

## 6. Intended code/ownership changes

`.github/workflows/windows-process-launcher.yml`, optionally a bounded test-only research workflow, and `architecture/testing.md`. No production modules, storage, DTOs, config, migrations or installer changes. Treat C011 deliberately known-negative tests separately from required positive regressions, with exact expected-outcome documentation. A negative research result is not a green enforcement test.

## 7. Work packages

A. Inventory each YAML step/target/filter, observed selected count, historical closure and intended required versus opt-in research classification. Remove duplicate ordinary steps and stale no-op filters.

B. Separate known-negative C014 from the required launcher path without deleting the test. Keep C011 security counterexamples directly reproducible. Reject unconditional skips and arbitrary success-on-failure.

C. Run the full Windows process-launcher workflow on **the implementation SHA**, after C014 is no longer gating. Verify each intended launcher/Job/ACL/status/policy/shell suite ran. If a real distinct regression now fails, investigate it narrowly; do not silently weaken the test.

D. Update testing docs and closure with before/after workflow links, selected counts and explicitly unrun research cases.

## 8. Failure/restart/concurrency

Use private per-test roots, serial `--test-threads=1` where required; no cross-run shared ACL fixtures. A missing privileged Windows runner yields blocked evidence, not pass. Failure-injected Job assignment must terminate the suspended child. A newly discovered security escape is a separate blocker.

## 9. Compatibility

No API, protocol, storage or config change. Preserve Rust 1.89 MSRV and Unix/macOS behavior. C014 and C011 historical closures remain intact.

## 10. Tests

Mandatory positive: `windows_process_launcher`, `windows_restricted_managed_process`, `windows_acl_lease` and the existing focused suspended-target, restricted stdio, environment, status-frame, AppContainer ordering and native-shell/policy tests. Qualified `windows_sandbox_adversarial` cases must run with explicit expected outcome, not known-failing C011 assertions under a green label. Verify nonzero selections. C014 must remain runnable independently and explicitly negative.

## 11. Commands

`cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `git diff --check`; `bash scripts/verify.sh quick`.

On Windows MSVC:
`cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_restricted_managed_process -- --nocapture --test-threads=1`
`cargo test --locked -p codegg --features windows-sandbox-test-support --test windows_acl_lease -- --nocapture --test-threads=1`

Also run the **entire** Windows process-launcher job and report every selected test/filter.

## 12. Documentation

`architecture/testing.md`, roadmap/registry when evidence lands; new `plans/closure/windows-native-sandbox/023-status.md`.

## 13. Acceptance

At exact SHA, required Windows process-launcher CI passes with all intended positive suites actually executed; C014 is neither misreported as a pass nor blocking those suites; duplicate invocations removed; no permission regression, skipped test illusion or production admission.

## 14. Stop conditions

Further failures imply new security behavior, new OS privilege requirements or a different architecture; report and register a focused blocker rather than ignoring/redesigning unrelated code.

## 15. Closure evidence

Exact SHA, old and new CI run IDs, OS edition/build/UBR, step/test matrix and nonzero selected/passed/failed/skipped counts, retained C014 research command, and static/Unix guard results.

## 16. Handoff

CI reconciliation only. It unblocks **trustworthy CI**, not Windows filesystem isolation.
