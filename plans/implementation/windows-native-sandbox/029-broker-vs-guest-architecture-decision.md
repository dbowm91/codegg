# Windows Native Sandbox C029 — Architecture Decision: Broker versus Native Windows Guest

Status: implemented (no-admission disposition; Windows constrained execution remains unavailable)
Primary class: invariant/architecture
Repository baseline: `7a27bc7e6566616fc1b2b8a69637f12050d42843` on `work/windows-native-sandbox-foundation` (2026-10-13).
Source roadmap: `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0021-launch-authority-decision-gate`.
Architecture decision: `plans/adrs/ADR-0023-windows-privileged-launch-vs-guest-isolation.md` (rejected; no admission); research: `research/windows-native-sandbox/adr-0021-authority-options.md`.
Canonical references: `plans/000-long-term-specification.md` sections 4, 17, 27, 29; `plans/001-terminology-and-domain-model.md` section 8; `plans/003-planning-process.md`; ADR-0004, ADR-0016, ADR-0020.
Immutable evidence: C024, C026 and C027 status records.

## 1. Objective

Compare an opt-in privileged launcher with native Windows VM/guest isolation, select exactly one test-only candidate or retain Windows `Required` unavailable.

## 2. Readiness and dependencies

Requires C028 actual bounded phase/VM disposition, positive or negative. If VM unavailable a no-admission/blocked decision is valid, but broker choice cannot imply launch success. Proposed ADR-0023 requires explicit maintainer approval.

## 3. Current repo evidence

C027 returned Win32 1314 and token child timeout without real read proof. Windows Sandbox is single-instance, edition-limited and defaults clipboard/network ON; broker requires high-trust IPC, account/token and Job ownership.

## 4. Security invariants

No new privilege or product backend in C029; same `ManagedProcessService` launch authority, no original file ACL modifications, truthful obtained security. The maintainer decision leaves both alternatives terminally blocked and unselected.

## 5. In-scope / out-of-scope

Research, threat model, OS/toolchain/cost matrix, approved ADR disposition. Excludes writing broker code, spinning guests and modifying daemon IPC/public schema.

## 6. Required changes

Record the maintainer decision in ADR-0023. The decision is no admission: reject route selection, keep Windows constrained execution unavailable, and leave C030/C031 terminally blocked as unselected alternatives.

## 7. Ordered work packages

WP-A: C028 status and C019–C027 actual security matrix. WP-B: broker pipe/SCM/auth/impersonation/nonce/token trust and runtime OS privileges. WP-C: Windows Sandbox vs Hyper-V guest edition, native Git, image, mapped-folder/clipboard/network, concurrency and output transfer. WP-D: risk-owner review and one selection/no-admission, with recorded feasibility limits.

## 8. Failure, cancellation, restart and contention

No executable lifecycle in decision. Missing reviewer, VM or security evidence cannot be treated as success; maintain no-admission. Assign crash/restart recovery and concurrency to later candidate plan.

## 9. Compatibility and migration

No API/installer/protocol changes; document supported Windows editions and WSL2-as-Linux-only alternative.

## 10. Required tests

Threat-model negative test table for broker confused deputy/pipe impersonation failure/replay, guest host folder/clipboard/network leaks, human authorization, immutable read-deny object, resource/concurrency. Reviewer signs actual unresolved severities.

## 11. Verification commands

Docs only: `git diff --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`; audit exact C028 workflow/closure, do not represent as native C029 test.

## 12. Documentation updates

ADR-0023 rejection/no-admission decision, C029 closure, and updated registry/roadmap terminal blocked states. No trust-boundary implementation or production path is authorized.

## 13. Acceptance criteria

Explicit no-admission recorded after human review; honest security/OS/cost matrix and terminal disposition of both unselected candidates; production remains unavailable.

## 14. Stop conditions

Reject if broker accepts arbitrary agent EXE/SID/ACL or can accidentally execute elevated; reject guest if host secret shares/clipboard/network cannot be isolated; no default FullHost fallback or unverified reviewer.

## 15. Closure evidence

Maintainer no-admission decision, C028 outcome, rationale for not selecting either candidate, terminal plan states and no production admission.

## 16. Handoff

C030 and C031 remain terminally blocked and unselected. Reconsideration requires a new explicit architecture decision and fresh qualified evidence; this decision does not reopen C025/C026, qualify M004, or authorize a production backend.

Closure: `plans/closure/windows-native-sandbox/029-status.md`.
