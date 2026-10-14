# Windows Native Sandbox C044 — Compiled native two-hop launch and read-boundary diagnostic

Status: implemented (negative bounded native feasibility) — hosted run 38468468249 at `76e9e9c9`: topology fully verified, static native child alive 30 s without entry (STILL_ACTIVE); no read-denial claimed (closure `plans/closure/windows-native-sandbox/044-status.md`)
Class: infrastructure (test-only candidate)
Baseline: main a7b116af3243623099b80cc200b843a36b0ce3d2 (refresh exact SHA before implementation).
Source roadmap: plans/subsystems/windows-native-sandbox-roadmap.md (ADR-0029 successor addendum).
Authority: ADR-0004, ADR-0016, ADR-0029 (bounded test-only), proposed ADR-0030 is NOT product authorization.
Dependencies: C032 historical negative; ADR-0029 bounded test-only; C042 interface/soft
Closure target: plans/closure/windows-native-sandbox/044-status.md.
General verification: git diff --check; python3 scripts/check_adr_integrity.py; python3 scripts/check_sandbox_contract.py; python3 scripts/check_execution_ownership.py; bash scripts/verify.sh quick. Use exact-SHA native Windows runs for all platform/security claims and record unrun limitations.

## Objective and historic blockers

C032 ran PowerShell as inner restricted child, successfully created/suspended/Job-assigned/resumed it, but never observed user entry within 30 seconds. Do not confuse PowerShell/.NET/loader startup with restricted-token enforcement. Test the identity/token distinction using a tiny compiled native runner and native target before reintroducing interpreter or developer tools.

## Work packages

A. Create isolated Rust 1.89-compatible or minimal C helper pair in research/windows-native-sandbox/ with pinned build and no product installer. Stage one trusted signed or hash-pinned runner in a disposable, independently verified account-owned directory with an immutable manifest, one-shot nonce and minimal inherited descriptors. First hop uses CreateProcessWithLogonW (dedicated standard user); second uses a restricted form of its own primary token with CreateRestrictedToken and CreateProcessAsUserW. Capture required token privileges, error codes, SID/restricting groups and verified Job/suspend/resume state.
B. Enter the native child immediately and emit nonce-bound proof before loading PowerShell, Python or Git. Validate actual child SID/token and a same-file broad-read baseline that the first-hop ordinary account can read, then attempt the same read under restricted child. Include allowed independent stage write and denied host write, direct/alias/hardlink/ADS/reparse routes. If child fails before entry, stop and diagnose bounded loader/privilege/environment errors rather than counting it as read-denied.
C. Test inherited stdio, arbitrary inherited/duplicate process/token handles, daemon IPC substitute, nested Jobs, descendants, cancellation, timeout and cleanup; no host-secret or global original-tree ACL mutation. Use only an administrator-consented disposable synthetic VM for temporary local user creation, remove accounts/profile/fixtures, record negative teardown evidence.
D. After native helper entry proves positive, run PowerShell/cmd, Git, native tool and offline Cargo compatibility as separately measured steps. If a limiting SID requires wide original-user ACEs or special host setup, close candidate negative or request a separate ADR; no opportunistic relaxing of existing invariant.

## Acceptance / stop

Strict-positive test-only requires same-file ordinary-read allowed/restricted-read denied with actual token proof, developer-tool compatibility, independent allowed stage writes, no host descriptor drift and deterministic Job cleanup. Alternative closure may demonstrate **write-only** confinement but must not be promoted to existing strict ReadOnly/WorkspaceWrite; ADR-0030 acceptance would still be required to discuss a weaker explicit profile. One bounded helper startup correction permitted, then issue honest positive/negative/blocked closure. Never install persistent sandbox account or enable production Required.
