# Windows Native Sandbox C027 — Restricted Primary-Token Win32 Interop Corrective

Status: implemented; closed (negative bounded feasibility)

Baseline: `ba04a4875587cd689220267a8ede419f9d2f5f81` on `work/windows-native-sandbox-foundation`; 2026-10-13.

Planning authority: `plans/003-planning-process.md`; `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-post-c026-launch-interop-corrective`; `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`; `plans/adrs/ADR-0016-cross-platform-sandbox-launch-authority.md`; `plans/adrs/ADR-0020-windows-native-sandbox-after-appcontainer-feasibility.md`. Long-term requirements: `plans/000-long-term-specification.md` ``4, 17, 27, 29` and `plans/001-terminology-and-domain-model.md` `8`.

Predecessor: C024 `plans/implementation/windows-native-sandbox/024-dedicated-principal-token-and-host-read-boundary.md`; immutable failure evidence `plans/closure/windows-native-sandbox/024-status.md`; C025 `025-status.md` closed without execution; C026 `026-status.md` closed no-admission.

Primary class: security invariant / infrastructure.

## 1. Objective

Resolve or isolate a concrete test-only `CreateProcessAsUserW` P/Invoke string-marshaling defect, establish a **running unrestricted primary-token API control** under a disposable Windows account, then—only after that passes—attempt a genuine read-restricting primary-token child and real file-read denial. Produce one finite positive/negative/blocked native verdict, not production admission.

## 2. Why ready

C023 launcher CI is closed (48/48 selected tests on run `38359288381`). C024 run `38362309506`, SHA `645f24a1`, established an ordinary sandbox-account child **could read** a synthetic host file with `Users`/`Authenticated Users`/`Everyone` read rights, but failed to launch any genuine restricted child. `CreateProcessAsUserW` returned errors 123/2; unrestricted `CreateProcessWithTokenW` timed out; restricted `CreateProcessWithTokenW` returned error 5. No actual read-denial result exists.

In `scripts/windows_dedicated_user_read_boundary.ps1`, C# declares `[DllImport("advapi32.dll", SetLastError = true)]` for `CreateProcessAsUserW` while invoking the native UTF-16 `W` API. .NET `DllImport` defaults to ANSI marshaling. This is an **observed ABI mismatch** and a plausible contributor to errors 123/2, not proof that all launches will succeed after correction. `CreateProcessWithTokenW` already declares Unicode; its independent errors must be investigated without conflation.

Microsoft references: https://learn.microsoft.com/en-us/dotnet/framework/interop/specifying-a-character-set ; https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessasuserw ; https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createprocesswithtokenw ; https://learn.microsoft.com/en-us/windows/win32/api/securitybaseapi/nf-securitybaseapi-createrestrictedtoken .

## 3. Invariants

- Production Windows `SandboxRequest::Required` remains unavailable; no filesystem/network `Enforced`, no automatic uncontained/FullHost fallback, backend registration, network claim, or privilege change in production.
- Only test-owned synthetic credentials/files, disposable account/stage, and an operator-consented administrator-capable Windows runner or isolated VM. Never mutate original user/source workspace DACLs, SACLs, ownership, hardlinks, system devices or registry global grants.
- The restricted **primary** token must originate from the disposable sandbox account, not the privileged operator. `WRITE_RESTRICTED` is write-only restriction and does not establish denied reads.
- Child remains suspended until token/Job provenance and handle inheritance are verified; kill-on-close Job owns descendants and cannot be inherited by sandbox child.
- Historical C024/C025/C026 negative/non-executed closures remain immutable; a new result does not retroactively change them.

## 4. Scope and non-goals

Allowed: narrowly correct `scripts/windows_dedicated_user_read_boundary.ps1` or create sibling test-only launcher script, build a small trusted native fixture executable, update `.github/workflows/windows-dedicated-user-feasibility.yml` and focused research/testing documentation. Reuse the actual Windows-hosted fixture. Excluded: production `src/managed_process.rs` and backend changes, account installer/elevated broker callable by agents, firewall/M005, global ACL fixes, broad workspace projection, MXC retries, unrelated branch work.

## 5. Work package A — ABI/marshaling correctness

1. Declare `CreateProcessAsUserW` with `[DllImport("advapi32.dll", CharSet=CharSet.Unicode, ExactSpelling=true, SetLastError=true)]`. Audit `STARTUPINFOW` struct layout/size/`cb`, `PROCESS_INFORMATION`, native `BOOL`, `StringBuilder` mutable UTF-16 `lpCommandLine`, entry-point identity and all `W` P/Invokes.
2. Supply a trusted absolute `lpApplicationName`, correctly quoted command line, stable account-accessible cwd and controlled environment. If environment is provided explicitly in Unicode, set `CREATE_UNICODE_ENVIRONMENT`; never inherit operator credentials as a convenience.
3. Capture `Marshal.GetLastWin32Error()` immediately after native failure, before cleanup/other P/Invoke. Distinguish API launch failure, child startup failure, timeout, actual child read failure and merely missing output. Add an ABI regression check to prevent implicit ANSI reintroduction.

**Gate A:** verified actual UTF-16 ABI and executable/argument/struct contract. Fixing a declaration alone is not completion.

## 6. Work package B — Positive unrestricted token-taking control

1. Acquire and inspect the *disposable account's* primary token (`TokenUser`, `TokenType`, relevant groups/rights `TOKEN_QUERY`, `TOKEN_DUPLICATE`, `TOKEN_ASSIGN_PRIMARY`).
2. Launch a deliberately minimal, bounded trusted native helper under `CreateProcessAsUserW` using the **unrestricted primary token**, keeping `CREATE_SUSPENDED` → Job assignment/verification → `ResumeThread`. Assert child exits, reports the expected SID, reads the **unchanged broadly readable synthetic host file** and can use private stage.
3. As a separately labeled control, test `CreateProcessWithTokenW` with the same minimal executable. Report `SeIncreaseQuotaPrivilege`, `SeAssignPrimaryTokenPrivilege`, `SeImpersonatePrivilege` and token access rights, profile/HKCU initialization, desktop/window-station access, DLL lookup and working directory. Distinguish Win32 error 5/2/123/1314 and timeout; none means read denial.
4. Permit only one focused parameter/privilege diagnosis after corrected ABI. Do not repeatedly change flags, relax host ACLs, swap the token identity or fall back to unrestricted `CreateProcessWithLogonW` to claim this gate.

**Gate B:** an unrestricted token-taking API launches the native fixture, produces actual account/token and Job evidence and exits without timeout. Otherwise close C027 **negative** or **blocked** if a necessary isolated host/consent is unavailable; do not claim read restriction.

## 7. Work package C — Real restrictive token and same-file contrast

Only after Gate B, create a **non-`WRITE_RESTRICTED` restricting-SID** token from the same account token, verifying it is a primary token and that `TokenRestrictedSids` and the limiting SID are present **in the child**, not only parent.

Launch exactly the same minimal helper under the same executable/cwd/environment/Job discipline, granting any additional needed read/write only to newly created stage/runtime test objects. Record **actual** Win32 file open/read result against the *same unchanged host file* that the normal account can read. The restricted child must be denied that read while permitted stage read/write succeeds. Keep privately protected file as a separate control; record file ID, hash, DACL/owner/control bits before and after.

Only if this contrast passes, smoke-test native shell and Git (and individually claim Python/Cargo only when run); then selectively exercise C019 direct/hardlink/junction/ADS/case, unauthorized sibling, IPC/inherited handles, descendant/Job observations. Never label missing probes as denied.

**Gate C:** real child reads allowed stage and runs a useful tool subset, genuinely cannot read broad-group host secret, retains correct SID/Job. A successful forbidden read, broad user-profile/cache grant or inability to run minimum toolchain is a negative finding.

## 8. Work package D — Native evidence and decision

Record implementation SHA; Windows edition/build/UBR, privilege/session/profile conditions; per-mode API, exact native errors/timeout/exit, token user/restricting SIDs, child-read results, original fixture DACL/metadata and hashes, Job teardown, synthetic account removal; actual nonzero test selection counts and workflow URL. The GitHub workflow can be green when **evidence collection** succeeded but must not report an unlaunched/failed restriction as a passing security test. Keep C014 known-negative experiment optional and C023 mandatory launcher regressions running independently.

Close as **strict-positive bounded feasibility**, **closed-negative**, or **blocked**; create `plans/closure/windows-native-sandbox/027-status.md`. A strict-positive result authorizes *new planning consideration only*: never reopen closed-without-execution C025 or no-admission C026 automatically, and never unblock C011/M004–M006 or a production `Required` path.

## 9. Lifecycle, cancellation and contention

On setup failure terminate suspended child, wait/reap and close Job/process/thread/token handles; no `ResumeThread` before required checks. Deadline every control and kill all Job descendants on timeout. Unique account SID, stage and Job per run; prevent name collision/foreign-account deletion; assert account/resources removed only when owned. Never confuse normal `finally` cleanup with reboot/crash recovery—those remain later gates, not C027 requirements.

## 10. Migration and compatibility

No production schema, config, protocol, MSRV, installer, daemon launcher or global Windows policy changes. Preserve Unix/macOS regressions and existing approval/sandbox separation. Windows Server 2025 data does not establish Windows 11 support. Network containment unclaimed.

## 11. Verification

Run native ABI/entry-point assertion and minimal control plus restricted fixture in Windows-hosted test workflow, with nonzero selected tests, exact SHA and per-mode output.

`pwsh -NoProfile -File scripts/windows_dedicated_user_read_boundary.ps1` or a separate C027 test script (use declared operator/admin consent; capture and classify results). `cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1`. Run C023 mandatory Windows launcher CI if its workflow or fixtures change.

Cross-host: `git diff --check`; `cargo fmt --all -- --check`; `python3 scripts/check_sandbox_contract.py`; `python3 scripts/check_execution_ownership.py`; `bash scripts/verify.sh quick`. Do not claim tests that did not exist, run, or select any cases.

## 12. Documentation

C027 closure, `research/windows-native-sandbox/c023-c026-corrective-assessment.md` (new post-closure section), subsystem roadmap and registry. Preserve `plans/closure/windows-native-sandbox/024-status.md`, `025-status.md` and `026-status.md` unchanged.

## 13. Strict acceptance and stop criteria

Must prove (A) UTF-16 ABI correct, (B) real **unrestricted** token-taking API child under expected disposable-account SID and Job reads the broad-readable file, (C) real read-restricted **primary-token** child denied read to that very same file while stage/shell/Git works, (D) fixture security and identity unchanged, no leaked Job/handles/account. A launch failure is not access denial. Stop after bounded control failure, hostile read, privilege/broker requirement exposing operator power or unsafe host-wide ACL grant. If no eligible host, report blocked rather than passing.

## 14. Handoff boundaries

Implement only this test-only diagnostic. Any subsequent account lifecycle/recovery, independent security review or production ADR needs a separately approved handoff following new strict evidence; **do not** silently reclassify the historical C024/C025/C026 dispositions. Leave unrelated branch diff for separate review.
