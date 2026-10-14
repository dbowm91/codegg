# Windows Native Sandbox C041 — Mainline Windows CI and test-only build boundary

Status: implemented; closed (closure `plans/closure/windows-native-sandbox/041-status.md`, implementation `edde2bebe`)
Class: infrastructure
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0029 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0029 (bounded test-only), proposed ADR-0030 is NOT product authorization.
Dependencies: C040 closed; no ADR-0030 or production security decision
Closure target: plans/closure/windows-native-sandbox/041-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective and existing evidence

Restore routine Windows launcher regression coverage after PR #111 merged and prevent accidentally shipping a binary with experimental Windows restricted-launch behavior. The existing .github/workflows/windows-process-launcher.yml still triggers push only on work/windows-native-sandbox-foundation, plus workflow_dispatch. Cargo.toml contains windows-sandbox-test-support, and src/managed_process.rs routes Required into test-only launch under cfg(windows, feature = windows-sandbox-test-support), rather than cfg(test) exclusively. These are maintenance and packaging boundaries, not a production sandbox.

## Bounded work packages

A. Inspect GitHub workflows and the repository release/packaging paths; add main + relevant pull_request triggers, path filters for production Windows process/policy changes, and concurrency guards. Keep historical C014 negative reproduction opt-in (manual boolean) and non-release hosted Windows host permission assumptions explicit. Avoid running all costly research workflows on ordinary PRs.
B. Add a deterministic build/release guard asserting that distributed Windows binaries cannot enable windows-sandbox-test-support. Include Cargo feature resolution checks in release workflows and test an intentionally enabled feature as a negative fixture. Decide whether feature should be forbidden in release via build validation or a test-only target boundary; do not remove needed native forensic tests.
C. Add exact-head required Windows process/Job/cancellation/fail-closed tests and Windows policy/shell tests to normal CI. Keep Windows Server hosted tests separate from Windows 11 ARM MXC eligibility jobs; no cross-host security inference.
D. Update C040 references in roadmap/registry as historical at its old exact SHA, noting PR #111 is now merged. Preserve closure text. Write C041 closure evidence and update registry status only upon verified implementation.

## Acceptance / failure

An ordinary Windows-affecting PR triggers Windows launcher tests; a mainline Windows-affecting push also does. A feature-enabled shipping artifact is rejected by an automated guard. No test-only backend becomes production-available, no existing negative tests are silently skipped, and no existing Unix CI is broken. Native workflow failures and host ineligibility are reported distinctly. No user-visible schema or permissions change, no persistent account/ACL/firewall side effect.
