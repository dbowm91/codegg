# Windows Native Sandbox C027 — Closure Status

Status: closed (negative bounded feasibility; no unrestricted token-taking child control)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/027-restricted-primary-token-win32-interop-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-post-c026-launch-interop-corrective`

Repository baseline reviewed: `eb1509b96899fb7db1da097c913139e13d03cdc5` (exact Windows evidence SHA)

Implementation commits:

- `5c0b80a4` — add the bounded native probe and Unicode token-launch harness.
- `735bc570`, `47c8a72b`, `3dcf90b4`, `eb1509b9` — correct hosted-runner mask, compiler, warning, and fixture-encoding issues found by successive exact-SHA runs.

Windows workflow: [run 38369937432](https://github.com/dbowm91/codegg/actions/runs/38369937432), exact SHA `eb1509b96899fb7db1da097c913139e13d03cdc5`.

## 1. Executive finding

C027 closes negative for the tested token-launch candidate. The corrected harness exercised the explicit UTF-16 `CreateProcessAsUserW` import, and the normal disposable-account baseline proved that the fixture, command line, primary-token observation, and file-access oracle worked. However, neither unrestricted token-taking API produced a usable child control: `CreateProcessAsUserW` returned Win32 1314 (`ERROR_PRIVILEGE_NOT_HELD`), and the unrestricted `CreateProcessWithTokenW` child did not complete within 15 seconds and was terminated through its kill-on-close Job. The restricting-SID child was therefore not attempted. There is no C027 read-denial evidence and no production admission.

The GitHub workflow succeeded because the bounded report and cleanup completed. Its success is not a passing token-launch or security result. The tested runner was Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`, UBR `33438`, with an administrator-capable `runneradmin` identity. `SeImpersonatePrivilege` was enabled; `SeIncreaseQuotaPrivilege` was present but disabled; `SeAssignPrimaryTokenPrivilege` was not listed. The harness did not alter runner privileges.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit UTF-16 ABI and stable application/argument contract | C027 harness at SHA `eb1509b`; hosted compilation and normal helper launch in run `38369937432` | pass for bounded diagnostic | `CreateProcessAsUserW` uses `CharSet.Unicode`, `ExactSpelling=true`, a trusted absolute application path, mutable `StringBuilder` command line, explicit target-token Unicode environment, explicit stage cwd, and immediate Win32 error capture. `STARTUPINFOW`/`PROCESS_INFORMATION` layouts were used by the successful normal-account baseline. |
| Ordinary disposable-account read baseline | `C027_REPORT.normal_account_baseline` in run `38369937432` | pass | Child primary token user matched the newly created account SID; broad-file read returned 0; private-file read returned 5; stage read/write returned 0. |
| Unrestricted `CreateProcessAsUserW` control | `create_process_as_user_unrestricted_account_control` | fail | Native API returned Win32 1314. No child was created. |
| Unrestricted `CreateProcessWithTokenW` control | `create_process_with_token_unrestricted_account_control` | fail | Child was created suspended, verified against the account primary token and Job, resumed, then exceeded the 15-second bound. The Job was terminated; no child report was produced. |
| Restricting-SID child and same-file denial | Gate B failure; `restricted_primary_token_launch=null` | not run | C027 stop condition prohibits treating launch failure as read denial. |
| Fixture identity, ACL and content preservation | report file IDs, SDDL SHA-256 values, and content hashes before/after | pass | Broad file ID remained `F0026AA0:0001000000000D8B`; private file ID remained `F0026AA0:0001000000000D8C`. Both SDDL fingerprints and file hashes were unchanged. |
| Disposable account and Job cleanup | hosted workflow logs and report | pass | Account removal was confirmed. Timed-out token child was terminated by its Job. |
| Shell/Git, aliases, IPC, inherited handles, descendants, network, production admission | C027 stop gate | not run / unavailable | No claims are made. |

## 3. Production implementation evidence

Changes are limited to a test-only native C probe, a disposable-account PowerShell harness, and the existing feasibility workflow. The harness checks the child token user/type and restricting SID, and verifies Job membership while the process is still suspended. Process creation disables handle inheritance. It supplies a target-token environment block and stage working directory, bounds each child, terminates timed-out Jobs, fingerprints both synthetic fixtures, and removes the uniquely named account and temporary root.

No production launcher, backend registration, policy, configuration, protocol, storage, installer, filesystem enforcement, or network enforcement changed. The historical C024, C025, and C026 closure records remain unchanged.

## 4. Verification executed

### Commands run

```text
PowerShell parser and Add-Type compilation for scripts/windows_restricted_primary_token_feasibility.ps1
x86_64-w64-mingw32-gcc -std=c11 -Wall -Wextra -Werror -municode scripts/windows_restricted_primary_token_probe.c -o /tmp/c027-probe.exe -ladvapi32
python3 YAML parse of .github/workflows/windows-dedicated-user-feasibility.yml
python3 scripts/check_sandbox_contract.py
python3 scripts/check_execution_ownership.py
cargo test --locked -p codegg --test windows_process_launcher -- --nocapture --test-threads=1
bash scripts/verify.sh quick
git diff --check
GitHub Actions Windows dedicated principal feasibility, run 38369937432
```

### Results

- PowerShell parsing and C# `Add-Type` compilation passed.
- The native C probe cross-compiled with MinGW warnings treated as errors; the final Windows run also compiled it with MSVC `/W4 /WX`.
- Workflow YAML parsed; sandbox and execution-ownership guards passed; `git diff --check` passed.
- `cargo test` exited successfully but selected **0 tests** on this non-Windows host. It is recorded as compile/test-command evidence only, not Windows behavior.
- `bash scripts/verify.sh quick` passed.
- Windows run `38369937432` passed as report collection. Its report disposition is `negative-no-unrestricted-token-control`; no restricted child ran.
- Earlier exact-SHA attempts exposed and corrected harness-only issues (PowerShell unsigned-mask conversion, MSVC invocation/warnings, and UTF-8 fixture comparison). Those runs are superseded and are not used as security acceptance evidence; their cleanup logs confirmed removal of the disposable account.

## 5. Invariant review

- Windows production `SandboxRequest::Required` remains unavailable; filesystem/network `Enforced` is not claimed.
- No original workspace, user profile, system path, or host ACL was modified. ACL changes were confined to newly created synthetic fixtures and temporary stage.
- The restricted child was not launched. No failed launch is represented as denied file access.
- `WRITE_RESTRICTED` was not used. No token privilege was enabled or changed by the harness.
- C024/C025/C026 historical findings were not rewritten.

## 6. Failure and recovery review

All child launches were suspended until token and Job checks completed, used non-inherited handles, and were bounded. The timed-out unrestricted `CreateProcessWithTokenW` child was terminated through its Job and reaped. The uniquely named local account and temporary fixture root were removed in `finally`. C027 does not test parent crash/reboot recovery, account collision recovery, concurrency, or cancellation races; those remain outside this bounded negative result.

## 7. Migration and compatibility review

No schema, protocol, configuration, dependency, MSRV, installer, daemon launcher, or production behavior changed. Windows Server 2025 evidence does not establish Windows 11 support. No network-containment claim was added.

## 8. Security review

The disposable account could read the synthetic broad-group-readable fixture. The two unrestricted token-taking controls did not establish a functioning child oracle, so the actual restricted-token access check could not be evaluated. Win32 1314 and timeout are launch outcomes, not file-read denials. The candidate remains unqualified and rejected for admission; no privilege escalation, broad ACL grant, or fallback to unrestricted execution was attempted.

## 9. Documentation and operations

- Added `scripts/windows_restricted_primary_token_probe.c` and `scripts/windows_restricted_primary_token_feasibility.ps1`.
- Updated `.github/workflows/windows-dedicated-user-feasibility.yml` to run C027 on Windows hosted runners while leaving C014 optional and the C023 launcher workflow independent.
- Updated the Windows roadmap, registry, and `research/windows-native-sandbox/c023-c026-corrective-assessment.md` with this negative result.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Neither unrestricted token-taking API produced a usable child control on the tested hosted runner. | No restricted-token read boundary or useful-tool compatibility evidence exists; the candidate cannot be admitted. | Keep Windows constrained execution unavailable. Any new candidate requires a separately registered plan and a working unrestricted token control before access claims. |
| medium | The reason `CreateProcessWithTokenW` timed out after process creation was not isolated; `CreateProcessAsUserW` returned 1314 with the runner privilege set. | No launch API has been qualified for this disposable-account primary token on this runner. | Do not repeat flag/privilege/identity experiments under C027; any new diagnosis needs a bounded handoff. |
| low | Native shell/Git and alias/IPC/descendant matrices were not reached. | Broader compatibility and containment are unknown. | No denial or compatibility claim. |

## 11. Roadmap disposition

C027 is closed negative for this bounded token-launch candidate. Its strict-positive gate failed before any restricting-SID child read. No C011/M004–M006 or C015–C018 work is unblocked; no automatic successor plan is created. Any future token-launch candidate needs a separately registered and reviewed plan. Windows production `SandboxRequest::Required` and filesystem/network `Enforced` remain unavailable.

## 12. Registry updates

- Mark C027 closed (negative bounded feasibility) in the implementation plan, subsystem roadmap, and `plans/registry.md`.
- Record this closure and hosted report in the registry's recently closed work section.
- Re-audit blocked work: C027 was not a hard/interface dependency of another registered plan. C011/M004–M006 and C015–C018 remain blocked; no plan becomes ready.
