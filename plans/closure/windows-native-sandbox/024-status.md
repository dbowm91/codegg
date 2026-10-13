# Windows Native Sandbox C024 — Closure Status

Status: closed (negative candidate result; no viable restricted-token child launch was established)

Source implementation plan:

- `plans/implementation/windows-native-sandbox/024-dedicated-principal-token-and-host-read-boundary.md`

Source subsystem roadmap:

- `plans/subsystems/windows-native-sandbox-roadmap.md#adr-0020-corrective-qualification-after-c022`

Repository baseline reviewed: `645f24a1ff35e7b38db5580f1b09346d95af00b8` (exact C024 hosted evidence SHA)

Implementation commits or pull requests:

- `40de0257` — bounded per-mode launch outcomes, timeout termination, runner UBR capture.
- `645f24a1` — keep launch telemetry out of the result-object stream.
- Windows workflow run [38362309506](https://github.com/dbowm91/codegg/actions/runs/38362309506), exact SHA `645f24a1ff35e7b38db5580f1b09346d95af00b8`.

## 1. Executive finding

C024 closes negative for the tested dedicated-principal restricted-token launch candidate. The ordinary disposable account passed the control: its actual child token included `Everyone`, `BUILTIN\Users`, and `Authenticated Users`, it read the synthetic file whose DACL allowed those groups, it was denied the private-DACL control, and it wrote to its private stage. The broad and private fixture DACL fingerprints and secret hashes were unchanged.

No restricted-token child ran to an observable access-check result. `CreateProcessAsUserW` controls failed with Win32 123 and 2; the unrestricted `CreateProcessWithTokenW` control started but did not complete within the bounded 15-second wait and was terminated; the restricted-token `CreateProcessWithTokenW` attempt failed with Win32 5 (access denied). Therefore there is no read-denial result and no strict-positive evidence. Do not interpret launch failure as a demonstrated host-read denial or as a successful sandbox. The tested candidate is rejected because it could not launch a useful restricted child without further unbounded privilege/API or host-ACL workarounds.

Production Windows `SandboxRequest::Required` remains unavailable. C025 is abandoned under its explicit C024 hard gate; C026 records no admission. No source ACLs, profile ACLs, or host fixture ACLs were changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Actual standard-account identity and broad-read baseline | Run `38362309506`, `C024_REPORT.normal_account_baseline` | pass | `TokenUser` matched the newly created account SID; child groups included `Everyone`, `Users`, and `Authenticated Users`; direct broad-file read succeeded. |
| Private-DACL negative control and allowed stage write | Same report | partial | Private control returned a managed `MethodInvocationException`; stage write succeeded. The test did not capture a native access error for the private control. |
| True restricting-SID primary-token child | Same report, `restricted_primary_token_launch`, `restricted_create_process_with_token` | fail / negative candidate | `CreateProcessAsUserW` failed with Win32 2; `CreateProcessWithTokenW` failed with Win32 5. No child token report or read observation exists. |
| Distinguish launch failure from read denial | Same report, unrestricted controls | fail / negative candidate | `CreateProcessAsUserW` control returned Win32 123; unrestricted `CreateProcessWithTokenW` timed out and was terminated. The native-token launch path did not establish a functioning control child. |
| Preserve source fixture descriptors and contents | Same report, before/after SDDL SHA-256 and secret hashes | pass | Both fixture SDDL fingerprints and both file hashes matched. `host_acl_mutation=false`. |
| Tool, aliases, IPC, handle, descendant and network matrix | Plan §10; same report | not run | The launch gate failed before those probes; none are claimed denied. |
| Production admission | Script report | unavailable | `production_admission=false`; no network guarantee claimed. |

## 3. Production implementation evidence

Changes are confined to the test-only PowerShell feasibility script and GitHub workflow. No production process launcher, scheduler, storage, protocol, configuration, installer, or account-provisioning path changed. The script creates and removes a uniquely named disposable local user and temporary synthetic fixtures. The hosted run confirmed `principal_removed=true` and unchanged fixture metadata/content.

## 4. Verification executed

### Commands and hosted evidence

```text
bash scripts/verify.sh quick
git diff --check
GitHub Actions: Windows dedicated principal feasibility, run 38362309506
```

### Results

- `scripts/verify.sh quick` passed at the C024 script revision before the final telemetry-only `Write-Host` correction; that correction does not change production code or test semantics.
- `git diff --check` passed after the final correction.
- Run `38362309506` completed successfully as a report-producing workflow on Windows Server 2025 Datacenter, version `10.0.26100`, build `26100`, UBR `33438`. The dedicated-principal control job passed; the bounded read-boundary job emitted the report and cleaned up the account.
- The hosted workflow's green conclusion means the bounded experiment/report completed. The report disposition is `blocked-create-process-as-user-control`, and the candidate is negative for admission. It does not mean the restricted read test passed.
- Earlier diagnostic run `38362180297` on SHA `40de0257` recorded the same API outcomes but included progress strings in the returned object. It is superseded by the clean report on `645f24a1`.
- The C021 standard-user workflow `38362180297` also passed its normal disposable-principal control. No strict restricted-token test passed.

## 5. Invariant review

- Production Windows `SandboxRequest::Required` remains unavailable; no filesystem or network `Enforced` outcome is claimed.
- `WRITE_RESTRICTED` is not treated as read confinement. Its child did not launch; no write-only result is promoted.
- The ordinary account result is explicitly separated from the unlaunched restricted-token result.
- No original workspace, profile, or broad-readable fixture descriptor was altered. No grants were added to Windows system roots or user caches.
- No fallback to unrestricted `CreateProcessWithLogonW` is treated as restricted-token evidence.

## 6. Failure and recovery review

The process children were suspended, assigned to a kill-on-close Job, and only then resumed. The unrestricted `CreateProcessWithTokenW` control exceeded the 15-second bound; the harness terminated its Job/process before returning. The disposable principal was removed and the temporary root cleaned. C024 did not test parent crash, VM restart, orphan recovery, concurrency, cancellation races, or account-name/SID collision; these remain outside the rejected candidate's qualification.

## 7. Migration and compatibility review

No product API, schema, configuration, protocol, dependency, MSRV, or customer-host compatibility claim changed. Evidence is Windows Server 2025 only and does not establish Windows 11 behavior.

## 8. Security review

The ordinary account can read the fixture because its broad-group DACL permits it. The restricted token was created from the disposable account token with a distinct restricting SID, but the candidate launch APIs did not produce a running restricted child. The required two-process read contrast is therefore absent. This is a negative candidate/admission result, not a positive access-control proof or an observed secret leak. The tested approach stops here; no source ACL grant or further token-launch variant is authorized by C024.

## 9. Documentation and operations

- Updated `architecture/testing.md` and `.github/workflows/windows-process-launcher.yml` for the C023 required-vs-research split; the C024 workflow records the runner privilege and OS evidence.
- Added `scripts/windows_dedicated_user_read_boundary.ps1` as a test-only bounded experiment with explicit per-mode errors, timeout termination, fixture fingerprints, and account cleanup.
- Updated the Windows sandbox roadmap and registry to retain the no-admission posture and gate C025/C026 from this result.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No working restricted-token child launcher was established; no same-file broad-read denial was measured. | Dedicated-principal confinement is not a viable or qualified Windows backend. | Keep Windows constrained execution unavailable. Any new candidate requires a new bounded plan and working native-token control before access claims. |
| medium | Shell/Git/Python/Cargo, aliases, IPC, inherited handles, descendants, network and Windows 11 remain unmeasured under a restricted child. | No compatibility or broader threat-boundary claim is supported. | Do not infer denials from unavailable probes. |
| low | Private-control failure surfaced as `MethodInvocationException`, not a native access status. | The test does not provide a detailed private-control error code. | Preserve as a harness limitation; it does not change the broad-read baseline or candidate disposition. |

## 11. Roadmap disposition

C024 is closed negative for the tested restricted-token candidate because the hosted runner could not execute a useful candidate child through the bounded native APIs. No broad-readable read-denial proof exists. C025's explicit strict-positive prerequisite failed, so lifecycle/recovery work is abandoned for this candidate. C026 records no admission without an independent positive-candidate review. Existing C011/M004–M006 and C015–C018 gates remain blocked.

## 12. Registry updates

- Record C024 as closed negative candidate with exact Windows run `38362309506` and SHA `645f24a1`.
- Record C025 as closed without execution under its C024 hard-gate stop condition.
- Close C026 with a no-admission disposition; no positive recommendation or production ADR is proposed.
- Keep Windows production `Required` unavailable and filesystem/network enforcement unclaimed.
